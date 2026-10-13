# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy", "osmium>=4.0", "pyogrio>=0.10", "shapely>=2.1"]
# ///
"""判定用データ（FlatGeobuf）を書き出す。

国道マップ（nanase/kokudo-map）の生成物と、その元になった pbf・N03 を読むだけで、
あちらには何も書かない。書き出す物は三つある。

  roads.fgb         国道・都道府県道の way。路線番号、橋・トンネル、自動車専用の印
  michi-no-eki.fgb  道の駅の位置と名前
  admin.fgb         市区町村（政令市は区）の面。簡略化済み

使い方:  uv run pipeline/export_geo.py --kokudo <国道マップのフォルダ> [--out data/geo]
         uv run pipeline/export_geo.py --only admin      # 一部だけ作り直す

国道マップのフォルダは、環境変数 KOKUDO_MAP_DIR でも渡せる。
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from collections import defaultdict
from pathlib import Path

import numpy as np
import osmium
import pyogrio.raw
import shapely

# 面の簡略化で許す、元の輪郭からのずれ（度）。約 10 m。境の位置がこれだけ動いても、
# 字幕のタイミングには影響しない。
SIMPLIFY_DEGREES = 0.0001
# これより小さい面は捨てる（度の 2 乗）。約 1 ha。道の無い岩礁が大半である。
MIN_AREA_DEGREES2 = 1e-6
# 同じ道の駅とみなす距離（度）。約 300 m。駐車場と建物が別々に名前を持つ。
SAME_STATION_DEGREES = 0.003

# 橋・トンネルの名前らしい `name`。`name` の多くは道路名なので、形で見分ける。
STRUCTURE_NAME = re.compile(r"(橋|トンネル|隧道|洞門|覆道|シェッド|シェルター)$")
# `tunnel` の値のうち、覆道として扱うもの。
SHED_VALUES = {"avalanche_protector", "covered", "rock_shed", "snow_shed"}
# 道の駅そのものではない、同じ名前を持つ物。
NOT_A_STATION = {
    ("highway", "traffic_signals"), ("highway", "bus_stop"), ("highway", "crossing"),
    ("public_transport", "platform"), ("public_transport", "stop_position"),
    ("amenity", "bicycle_rental"), ("amenity", "toilets"),
}


def log(started: float, message: str) -> None:
    print(f"[{time.time() - started:5.0f}s] {message}", flush=True)


def write(path: Path, geometry_type: str, geometries, fields: dict[str, np.ndarray]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pyogrio.raw.write(
        path,
        geometry=shapely.to_wkb(np.asarray(geometries, dtype=object)),
        field_data=list(fields.values()),
        fields=list(fields.keys()),
        geometry_type=geometry_type,
        crs="EPSG:4326",
        driver="FlatGeobuf",
    )
    print(f"        {path.name}: {len(geometries):,} 件、{path.stat().st_size / 1e6:.1f} MB", flush=True)


# ---------------------------------------------------------------- roads ---
def read_kokudo_ways(kokudo: Path) -> dict[int, dict]:
    """国道マップの地域別 GeoJSON から、way ごとの線形と路線番号を集める。

    国道の地域は矩形で重なるので、同じ way が複数の地域に出る。同じ way が国道と
    都道府県道の両方に出ることもある。どちらも way の id で束ねる。
    """
    ways: dict[int, dict] = {}
    for key, sub in (("national", "regions"), ("prefectural", "prefectural")):
        files = sorted((kokudo / "build" / sub).glob("*.geojson"))
        if not files:
            sys.exit(f"{kokudo / 'build' / sub} に GeoJSON がありません")
        for file in files:
            for feature in json.loads(file.read_text(encoding="utf-8"))["features"]:
                p = feature["properties"]
                way = ways.setdefault(p["id"], {
                    "coords": feature["geometry"]["coordinates"],
                    "national": set(), "prefectural": set(),
                    "kind": p["kind"], "former": 0,
                })
                way[key].update(str(ref) for ref in p["refs_list"])
                way["former"] |= int(p["former"])
    return ways


def export_roads(kokudo: Path, out: Path, started: float) -> None:
    ways = read_kokudo_ways(kokudo)
    log(started, f"国道マップの way {len(ways):,} 本")

    tags: dict[int, dict[str, str]] = {}
    reader = osmium.FileProcessor(str(kokudo / "build" / "pbf" / "japan-latest.osm.pbf"), osmium.osm.WAY)
    for way in reader.with_filter(osmium.filter.KeyFilter("highway")):
        if way.id in ways:
            tags[way.id] = {t.k: t.v for t in way.tags}
    log(started, f"pbf でタグを引けた way {len(tags):,} 本")

    ids = sorted(ways)
    structure, structure_name = [], []
    for wid in ids:
        t = tags.get(wid, {})
        bridge, tunnel = t.get("bridge", "no"), t.get("tunnel", "no")
        if tunnel == "yes":
            kind, name = "tunnel", t.get("tunnel:name")
        elif tunnel in SHED_VALUES:
            kind, name = "shed", t.get("tunnel:name")
        elif bridge != "no":
            kind, name = "bridge", t.get("bridge:name")
        else:
            kind, name = "", None
        if kind and not name and STRUCTURE_NAME.search(t.get("name", "")):
            name = t["name"]
        structure.append(kind)
        structure_name.append(name or "")

    numeric = lambda refs: ";".join(sorted(refs, key=lambda r: (r.rsplit("-", 1)[0], int(r.rsplit("-", 1)[-1]))))
    # 点の数が way ごとに違うので、平らに並べて、どの way の点かを添える。
    counts = [len(ways[w]["coords"]) for w in ids]
    lines = shapely.linestrings(
        np.concatenate([np.asarray(ways[w]["coords"], dtype=float) for w in ids]),
        indices=np.repeat(np.arange(len(ids)), counts),
    )
    write(out / "roads.fgb", "LineString", lines, {
        "id": np.array(ids, dtype=np.int64),
        "national": np.array([numeric(ways[w]["national"]) for w in ids], dtype=object),
        "prefectural": np.array([numeric(ways[w]["prefectural"]) for w in ids], dtype=object),
        "kind": np.array([ways[w]["kind"] for w in ids], dtype=object),
        "former": np.array([ways[w]["former"] for w in ids], dtype=np.int32),
        "motorroad": np.array([int(tags.get(w, {}).get("motorroad") == "yes") for w in ids], dtype=np.int32),
        "structure": np.array(structure, dtype=object),
        "structure_name": np.array(structure_name, dtype=object),
    })


# -------------------------------------------------------- michi-no-eki ---
def station_name(name: str) -> str:
    """表記の揺れを揃える。「道の駅　大杉」「道の駅「鷲の里」」を同じ形にする。"""
    rest = re.sub(r"[\s　「」『』\"']", "", name.removeprefix("道の駅"))
    return f"道の駅 {rest}"


def export_stations(kokudo: Path, out: Path, started: float) -> None:
    found: dict[str, list[tuple[bool, float, float]]] = defaultdict(list)
    reader = osmium.FileProcessor(str(kokudo / "build" / "pbf" / "japan-latest.osm.pbf"))
    for obj in reader.with_locations().with_filter(osmium.filter.KeyFilter("name")):
        name = obj.tags.get("name", "")
        if not name.startswith("道の駅") or len(name) <= 3 or obj.is_relation():
            continue
        if any((t.k, t.v) in NOT_A_STATION for t in obj.tags):
            continue
        if obj.is_node():
            lon, lat = obj.lon, obj.lat
        else:
            points = [(n.lon, n.lat) for n in obj.nodes if n.location.valid()]
            if not points:
                continue
            lon, lat = (sum(axis) / len(points) for axis in zip(*points))
        rest_area = obj.tags.get("highway") in ("services", "rest_area")
        found[station_name(name)].append((rest_area, lon, lat))
    log(started, f"道の駅の名前 {len(found):,} 種類")

    # 名前ごとに 1 点へまとめる。休憩施設として描かれた物があれば、それを位置にする。
    stations = []
    for name, objects in found.items():
        chosen = [o for o in objects if o[0]] or objects
        stations.append((name, sum(o[1] for o in chosen) / len(chosen), sum(o[2] for o in chosen) / len(chosen)))
    # 「道の駅 しなの」と「道の駅 しなの物産館」のように、近くにある長い名前は施設の名前である。
    stations.sort(key=lambda s: len(s[0]))
    kept: list[tuple[str, float, float]] = []
    for name, lon, lat in stations:
        near = any(abs(lon - k[1]) < SAME_STATION_DEGREES and abs(lat - k[2]) < SAME_STATION_DEGREES for k in kept)
        if not near:
            kept.append((name, lon, lat))
    kept.sort()
    write(out / "michi-no-eki.fgb", "Point", shapely.points([(s[1], s[2]) for s in kept]), {
        "name": np.array([s[0] for s in kept], dtype=object),
    })


# ---------------------------------------------------------------- admin ---
def simplified(polygons: np.ndarray) -> np.ndarray:
    """隣り合う面が共有する境を保ったまま簡略化する。面の集まりとして正しくなければ、1 つずつ簡略化する。"""
    try:
        return shapely.coverage_simplify(polygons, SIMPLIFY_DEGREES)
    except shapely.errors.GEOSException:
        return shapely.simplify(polygons, SIMPLIFY_DEGREES, preserve_topology=True)


def export_admin(kokudo: Path, out: Path, started: float) -> None:
    zips = sorted((kokudo / "build" / "n03" / "2026").glob("*.zip"))
    if not zips:
        sys.exit(f"{kokudo / 'build' / 'n03' / '2026'} に N03 がありません")
    geometries, rows = [], []
    for archive in zips:
        _, _, wkb, columns = pyogrio.raw.read(
            f"/vsizip/{archive.as_posix()}/N03-20260101_{archive.stem}.shp", columns=["N03_001", "N03_004", "N03_005", "N03_007"], encoding="utf-8",
        )
        polygons = simplified(shapely.from_wkb(wkb))
        pref, muni, ward, code = columns
        for i, polygon in enumerate(polygons):
            # 所属未定地は市区町村の符号を持たない。境として出せないので捨てる。
            if code[i] is None or polygon is None or polygon.is_empty:
                continue
            for part in shapely.get_parts(polygon):
                if part.geom_type == "Polygon" and part.area >= MIN_AREA_DEGREES2:
                    geometries.append(part)
                    rows.append((code[i], pref[i], f"{muni[i] or ''}{ward[i] or ''}"))
        log(started, f"N03 {archive.stem}: 累計 {len(geometries):,} 面")
    write(out / "admin.fgb", "Polygon", geometries, {
        "code": np.array([r[0] for r in rows], dtype=object),
        "pref": np.array([r[1] for r in rows], dtype=object),
        "muni": np.array([r[2] for r in rows], dtype=object),
    })


EXPORTS = {"roads": export_roads, "eki": export_stations, "admin": export_admin}


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description="判定用データ（FlatGeobuf）を書き出す")
    parser.add_argument("--kokudo", type=Path, default=os.environ.get("KOKUDO_MAP_DIR"),
                        help="国道マップのフォルダ。読むだけで、何も書かない")
    parser.add_argument("--out", type=Path, default=Path("data/geo"))
    parser.add_argument("--only", default=",".join(EXPORTS), help="作る物。roads,eki,admin から選ぶ")
    args = parser.parse_args()
    if args.kokudo is None:
        parser.error("--kokudo か環境変数 KOKUDO_MAP_DIR で、国道マップのフォルダを指定してください")
    if args.out.resolve().is_relative_to(args.kokudo.resolve()):
        parser.error("書き出し先は、国道マップのフォルダの外にしてください")

    started = time.time()
    for key in args.only.split(","):
        if key not in EXPORTS:
            parser.error(f"--only に使えるのは {', '.join(EXPORTS)} です: {key}")
        log(started, f"{key} を書き出す")
        EXPORTS[key](args.kokudo, args.out, started)
    log(started, "完了")


if __name__ == "__main__":
    main()
