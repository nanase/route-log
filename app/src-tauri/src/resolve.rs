//! 回を、Resolve で読み込むソースのタイムライン（FCPXML）とマーカーの EDL に書き出す。

use std::path::{Path, PathBuf};

use routelog_core::RideDocument;
use routelog_core::resolve::{self, SourceTimeline};
use routelog_video::Ffprobe;
use serde::Serialize;

use crate::config::ConfigFile;
use crate::stops::Stops;
use crate::videos;

/// 書き出した結果。
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Exported {
    /// 書いた。
    #[serde(rename_all = "camelCase")]
    Written {
        /// 書いたファイルの名前
        fcpxml: String,
        edl: String,
        /// タイムラインの長さ
        length_ms: i64,
        /// 録画の中で抜いた停車の数と長さ
        cuts: usize,
        cut_ms: i64,
        markers: usize,
        /// 録画の外か、抜いた所にあって、マーカーにできなかったイベントの数
        unplaced: usize,
    },
    /// 同じ名前のファイルがあるので、書かなかった。
    Exists { names: Vec<String> },
}

/// 回 `episode_id` を、フォルダ `dir` に書き出す。名前は「走行の題 回の名前 ソース.fcpxml」と
/// 「走行の題 回の名前 マーカー.edl」で、`name` は画面に出している回の名前。
///
/// `overwrite` でなければ、同じ名前のファイルがあるときは書かずに知らせる。動画は読むだけで、
/// 何も書かない。
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "画面から渡す値と、受ける状態")]
pub async fn export_resolve(
    videos: tauri::State<'_, videos::State>,
    stops: tauri::State<'_, Stops>,
    config: tauri::State<'_, ConfigFile>,
    doc: RideDocument,
    episode_id: String,
    name: String,
    dir: String,
    overwrite: bool,
) -> Result<Exported, String> {
    let episode = doc
        .episodes
        .iter()
        .find(|e| e.id == episode_id)
        .ok_or("パートが見つかりません")?;
    let extent = doc
        .extent(episode)
        .ok_or("パートの範囲が正しくありません。終わりは、始まりより後の経由地にします")?;
    let timeline = videos
        .current()
        .filter(|t| !t.files().is_empty())
        .ok_or("この走行ログには動画が指定されていません")?;

    let stem = resolve::file_stem(&doc.ride.title, &name);
    let title = format!("{stem} ソース");
    let (fcpxml, edl) = (format!("{title}.fcpxml"), format!("{stem} マーカー.edl"));
    let dir = PathBuf::from(dir);
    if !overwrite {
        let names: Vec<String> = [&fcpxml, &edl]
            .into_iter()
            .filter(|name| dir.join(name).exists())
            .cloned()
            .collect();
        if !names.is_empty() {
            return Ok(Exported::Exists { names });
        }
    }

    let cuts = resolve::real_cuts(&timeline, |file| {
        stops
            .taken(file)
            .into_iter()
            .map(|c| (c.start_ms, c.end_ms))
            .collect()
    });
    let ffprobe = Ffprobe::find(config.get().ffmpeg_dir());
    let leads = config.get().marker_leads;
    let mut made = tauri::async_runtime::spawn_blocking(move || {
        // 範囲の動画だけを読む。1 本に 0.25 秒ほどかかる。
        let used = resolve::files_in(&timeline, &extent);
        let files = timeline.files();
        let paths: Vec<&Path> = used.iter().map(|&i| Path::new(&files[i].path)).collect();
        let mut media = vec![None; files.len()];
        for (&i, read) in used.iter().zip(ffprobe.media_all(&paths)) {
            media[i] = Some(read.map_err(|e| format!("{}: {e}", files[i].name))?);
        }
        SourceTimeline::new(&doc, &extent, &timeline, &media, &cuts).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    made.lead_markers(&leads);

    let write = |name: &str, text: String| {
        let path = dir.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    write(&fcpxml, made.fcpxml(&title))?;
    write(&edl, made.marker_edl(&title))?;
    Ok(Exported::Written {
        fcpxml,
        edl,
        length_ms: resolve::frames_to_ms(made.frames()),
        cuts: made.cuts,
        cut_ms: made.cut_ms,
        markers: made.marker_count(),
        unplaced: made.unplaced,
    })
}
