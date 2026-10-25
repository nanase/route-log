/**
 * クラウドとの自動の同期を、手元で動かした Worker に対して確かめる。
 *
 * 同期は 2 つの端末の間で確かめる。端末は、アプリのデータの置き場所（`ROUTELOG_DATA_DIR`）を
 * 変えて起動することで真似る。1 回の起動で 1 つの段を行い、段は `ROUTELOG_CHECK_STEP` で選ぶ。
 * 段は `STEPS` に並べた順に行う。名前の頭の a と b が、起動する端末である。
 */

import { invoke } from "@tauri-apps/api/core";
import type { BarState, SyncView } from "./sync";
import type { LonLat, RideDocument, RideSummary } from "./types";
import type { SyncRow } from "./ui";

interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  idle(timeoutMs?: number): Promise<boolean>;
}

interface PanelHandle {
  resolve(id: string, keep: "local" | "cloud"): Promise<void>;
  remove(id: string): Promise<void>;
  snapshot(): { rows: string[]; message: string };
}

interface AppHandle {
  create(rideDate: string): Promise<void>;
  /** その日付の走行を開く */
  open(rideDate: string): Promise<void>;
  /** 開いている走行の題を直す。保存も送信も待たない */
  setTitle(title: string): Promise<void>;
  doc(): RideDocument | null;
  /** 上の帯に出す状態 */
  bar(): BarState | null;
  view(): SyncView;
  /** 接続先を読み直して、クラウドと突き合わせる。起動のときと、接続先を保存したときに、アプリが行うこと */
  start(): Promise<void>;
  /** 走行の切り替えの板を開き、その口を返す */
  openRides(): Promise<PanelHandle>;
  closeRides(): void;
  /** 手元に保存してある走行 */
  list(): Promise<RideSummary[]>;
  /** 最後に出した知らせ */
  message(): string;
  /** 上の帯の下に出ている帯の見出し */
  notices(): string[];
}

/** 確認で作る走行の題の頭。前の確認が残した走行と見分ける。 */
const MARK = "同期の確認";
/** 誰も聞いていないポート。通信できない状態を作る。 */
const DEAD = "http://127.0.0.1:8798";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** 条件が成り立つまで待ち、かかった時間（ms）を返す。待ちきれなければ null。 */
async function until(done: () => boolean | Promise<boolean>, timeoutMs: number): Promise<number | null> {
  const start = performance.now();
  while (performance.now() - start < timeoutMs) {
    if (await done()) return Math.round(performance.now() - start);
    await sleep(100);
  }
  return null;
}

/** 上の帯に、いま出ている状態の字。 */
const shown = () => document.querySelector("header [role=status]")?.textContent?.trim() ?? "";

/** 上の帯の状態の移り変わりを記録する。返した関数を呼ぶと止まり、出た順の一覧を返す。 */
function watch() {
  const seen: string[] = [];
  const timer = setInterval(() => {
    const now = shown();
    if (seen.at(-1) !== now) seen.push(now);
  }, 30);
  return () => {
    clearInterval(timer);
    return seen;
  };
}

export async function runSyncCheck(apiUrl: string, step: string, map: MapHandle, app: AppHandle) {
  const steps: Record<string, unknown>[] = [];
  const captures: string[] = [];
  /** 確かめた結果を 1 つ記録する。`ok` が、期待どおりだったかである。 */
  const note = (name: string, expected: string, ok: boolean, actual: Record<string, unknown> = {}) =>
    steps.push({ step: name, expected, ok, ...actual });
  const capture = async (name: string) =>
    captures.push(await invoke<string>("capture_check", { name }).catch((e) => `写せない: ${e}`));

  /** 接続先を、アプリに知らせずに差し替える。通信が切れた・戻った状態を作る。 */
  const wire = (url: string) => invoke("set_sync_settings", { apiUrl: url, clientId: "", clientSecret: "" });
  /** クラウドにある走行の題。無ければ null。 */
  const cloudTitle = (id: string) =>
    invoke<RideDocument>("sync_fetch", { id }).then(
      (doc) => doc.ride.title,
      () => null,
    );
  /** 手元のファイルにある走行の題。無ければ null。 */
  const diskTitle = (id: string) =>
    invoke<RideDocument>("load_ride", { id }).then(
      (doc) => doc.ride.title,
      () => null,
    );
  const local = async (id: string) => (await app.list()).some((ride) => ride.id === id);
  /** この確認で作った走行。手元の走行のうち、題が合う、いちばん新しく直したもの。 */
  const mine = async () => {
    const rides = (await app.list()).filter((ride) => ride.title.startsWith(MARK));
    return rides.sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1))[0] ?? null;
  };
  const synced = () => app.bar() === "synced" && shown() === "同期済み";
  /** 題を直し、押さずに待って、同期済みになるまでの時間を返す。 */
  const retitle = async (title: string) => {
    await app.setTitle(title);
    return until(synced, 15_000);
  };
  /** 起動のときと同じに、クラウドと突き合わせる。済むまで待つ。 */
  const startUp = async () => {
    await app.start();
    await until(() => !app.view().checking, 30_000);
  };
  /** 確認で作った走行を見つけて開く。 */
  const openMine = async () => {
    const ride = await mine();
    if (ride === null) throw new Error("確認で作った走行が、手元にありません。段の順番を確かめてください");
    await app.open(ride.ride_date);
    return ride;
  };

  const STEPS: Record<string, () => Promise<void>> = {
    /** 端末 A。走行を作ると自動で送られること、続けて編集している間は送らないこと、通信できないときの動き。 */
    "a-create": async () => {
      await wire(apiUrl);
      await startUp();
      // 日付を毎回変えて、前の確認が残したクラウドの走行と重ならないようにする。
      const day = String(1 + (Math.floor(Date.now() / 1000) % 28)).padStart(2, "0");
      const date = `2031-01-${day}`;
      let stop = watch();
      await app.create(date);
      map.jump([138.66, 36.35], 12);
      await map.idle();
      map.click([138.62, 36.345]);
      await sleep(300);
      map.click([138.7, 36.36]);
      await sleep(300);
      const first = await retitle(`${MARK} 1`);
      const id = app.doc()?.ride.id ?? "";
      note("走行を作って編集する", "押さなくても、数秒後に「同期済み」になり、クラウドに同じ題がある", first !== null && (await cloudTitle(id)) === `${MARK} 1`, {
        seen: stop(),
        waitedMs: first,
        cloud: await cloudTitle(id),
      });
      await capture("synced");

      // 1 秒おきに 8 回直す。待ち時間（3 秒）より短い間隔なので、その間は送らない。
      stop = watch();
      for (let i = 1; i <= 8; i++) {
        await app.setTitle(`${MARK} 連続 ${i}`);
        await sleep(1000);
      }
      const during = await cloudTitle(id);
      const seenDuring = stop();
      const after = await until(synced, 15_000);
      note(
        "1 秒おきに 8 回、続けて題を直す",
        "直している間は「同期済み」にならず、クラウドは前の題のまま。止めると最後の題が送られる",
        during === `${MARK} 1` && !seenDuring.includes("同期済み") && (await cloudTitle(id)) === `${MARK} 連続 8`,
        { seenDuring, cloudDuring: during, waitedMs: after, cloudAfter: await cloudTitle(id) },
      );

      // 通信できない状態で直す。手元への保存は続けられる。
      await wire(DEAD);
      await app.setTitle(`${MARK} 通信できない間`);
      const offline = await until(() => app.bar() === "unsynced" && shown() === "未同期", 15_000);
      const told = app.message();
      await capture("unsynced");
      stop = watch();
      await app.setTitle(`${MARK} 通信できない間 2`);
      const kept = await until(async () => (await diskTitle(id)) === `${MARK} 通信できない間 2`, 5000);
      // 保存のあと、もう一度送ろうとして、また「未同期」に戻る。
      await until(() => shown() !== "未同期", 5000);
      const again = await until(() => app.bar() === "unsynced" && shown() === "未同期", 15_000);
      note(
        "通信できない間に 2 度直す",
        "「未同期」になり、控えめな知らせが出る。赤い帯は出ない。直した題は手元に保存され、送れなければ「未同期」に戻る",
        offline !== null && kept !== null && again !== null && told.includes("同期できません") && app.notices().length === 0,
        { waitedMs: offline, message: told, notices: app.notices(), disk: await diskTitle(id), seen: stop(), failure: app.view().failure },
      );

      // 繋がるようにして、何もせずに待つ。少し置いて（60 秒）もう一度試し、送られる。
      await wire(apiUrl);
      const retried = await until(synced, 90_000);
      note(
        "繋がるようにして、何もせずに待つ",
        "最初に失敗してから 60 秒ほどで自動で送り直し、「同期済み」になる",
        retried !== null && (await cloudTitle(id)) === `${MARK} 通信できない間 2`,
        { waitedMs: retried, cloud: await cloudTitle(id) },
      );

      // もう一度通信できなくしてから繋ぎ、今度は次の保存で送られることを確かめる。
      await wire(DEAD);
      await app.setTitle(`${MARK} 通信できない間 3`);
      await until(() => app.bar() === "unsynced", 15_000);
      await wire(apiUrl);
      const next = await retitle(`${MARK} A1`);
      note("繋がるようにして、すぐ直す", "次の保存のあと数秒で送られ、「同期済み」になる", next !== null && next < 20_000 && (await cloudTitle(id)) === `${MARK} A1`, {
        waitedMs: next,
        cloud: await cloudTitle(id),
      });
      steps.push({ ride: { id, date } });
    },

    /** 端末 B。初めて起動すると、クラウドの走行を自動で受け取ること。 */
    "b-receive": async () => {
      await wire(apiUrl);
      const before = await mine();
      await startUp();
      const ride = await mine();
      note("別の端末で起動する", "何も押さなくても、端末 A で作った走行が手元にでき、「同期済み」になる", before === null && ride?.title === `${MARK} A1` && synced(), {
        before,
        received: ride,
        opened: app.doc()?.ride.title ?? null,
        shown: shown(),
      });
      await app.openRides();
      await sleep(300);
      await capture("received");
      app.closeRides();
      await openMine();
      const sent = await retitle(`${MARK} B1`);
      note("端末 B で題を直す", "自動で送られる", sent !== null && (await cloudTitle(ride?.id ?? "")) === `${MARK} B1`, { waitedMs: sent });
    },

    /** 端末 A。端末 B で直した内容を受け取り、開いている走行を読み直すこと。 */
    "a-update": async () => {
      await wire(apiUrl);
      const ride = await openMine();
      const opened = app.doc()?.ride.title;
      await startUp();
      note(
        "走行を開いたまま、クラウドと突き合わせる",
        "端末 B で直した題を受け取り、開いている走行がその題に変わる。受け取ったことを知らせる",
        opened === `${MARK} A1` && app.doc()?.ride.title === `${MARK} B1` && (await diskTitle(ride.id)) === `${MARK} B1` && synced(),
        { before: opened, after: app.doc()?.ride.title, disk: await diskTitle(ride.id), message: app.message(), shown: shown() },
      );
      await capture("updated");
      // 次の段のために、送れないまま手元で直しておく。
      await wire(DEAD);
      await app.setTitle(`${MARK} A2`);
      const offline = await until(() => app.bar() === "unsynced", 15_000);
      note("通信できない間に、端末 A で題を直す", "「未同期」になる", offline !== null, { shown: shown() });
    },

    /** 端末 B。端末 A が送れずにいる間に、同じ走行を直して送る。 */
    "b-diverge": async () => {
      await wire(apiUrl);
      await startUp();
      const ride = await openMine();
      const sent = await retitle(`${MARK} B2`);
      note("端末 B でも題を直す", "自動で送られる", sent !== null && (await cloudTitle(ride.id)) === `${MARK} B2`, { waitedMs: sent });
    },

    /** 端末 A。送ろうとして、クラウドが別の場所で書き換えられていたと分かる。手元を選んで揃える。 */
    "a-conflict": async () => {
      // 前の段のまま、通信できない状態で起動する。そのあと、アプリに知らせずに繋がるようにする。
      await startUp();
      const ride = await openMine();
      await wire(apiUrl);
      await app.setTitle(`${MARK} A3`);
      const found = await until(() => app.bar() === "conflict" && shown() === "両方で変更", 15_000);
      note(
        "両方で直した走行を、端末 A が送ろうとする",
        "上書きせずに「両方で変更」になり、知らせが 1 度出る。手元もクラウドも、それぞれの題のまま",
        found !== null && (await cloudTitle(ride.id)) === `${MARK} B2` && (await diskTitle(ride.id)) === `${MARK} A3` && app.notices().length === 1,
        { waitedMs: found, notices: app.notices(), cloud: await cloudTitle(ride.id), disk: await diskTitle(ride.id) },
      );
      await capture("conflict");
      // 上の帯の「両方で変更」を押すと、走行の切り替えの板が開く。
      document.querySelector<HTMLButtonElement>("header [role=status] button")?.click();
      const opened = await until(() => document.querySelector("dialog[open]") !== null, 3000);
      const panel = await app.openRides();
      await until(() => !app.view().checking, 15_000);
      await sleep(300);
      await capture("conflict-panel");
      const rows = panel.snapshot().rows.filter((row) => row.includes(MARK));
      await panel.resolve(ride.id, "local");
      const settled = await until(synced, 15_000);
      note(
        "板で「手元で上書き」を選ぶ",
        "上の帯のボタンで板が開く。選ぶと、クラウドが手元の題になり、「同期済み」に戻って、知らせの帯が消える",
        opened !== null && rows.some((row) => row.endsWith(": conflict")) && settled !== null && (await cloudTitle(ride.id)) === `${MARK} A3` && app.notices().length === 0,
        { panelOpened: opened !== null, rows, message: panel.snapshot().message, cloud: await cloudTitle(ride.id), notices: app.notices(), shown: shown() },
      );
      app.closeRides();
      const sent = await retitle(`${MARK} A4`);
      note("揃えたあとで、また直す", "自動で送られる", sent !== null && (await cloudTitle(ride.id)) === `${MARK} A4`, { waitedMs: sent });
    },

    /** 端末 B。保存を待っている編集があるときに受け取りが来ても、上書きしない。クラウドを選んで揃える。 */
    "b-conflict": async () => {
      await wire(apiUrl);
      const ride = await openMine();
      // 題を直してすぐ、保存を待たずに突き合わせる。クラウドには、端末 A の新しい題がある。
      await app.setTitle(`${MARK} B3`);
      await startUp();
      const found = await until(() => app.bar() === "conflict" && shown() === "両方で変更", 15_000);
      note(
        "保存を待っている編集があるときに、クラウドの新しい内容が来る",
        "受け取りで上書きせず、「両方で変更」になる。画面と手元は直した題のまま、クラウドは端末 A の題のまま",
        found !== null && app.doc()?.ride.title === `${MARK} B3` && (await diskTitle(ride.id)) === `${MARK} B3` && (await cloudTitle(ride.id)) === `${MARK} A4`,
        { waitedMs: found, screen: app.doc()?.ride.title, disk: await diskTitle(ride.id), cloud: await cloudTitle(ride.id), notices: app.notices() },
      );
      const panel = await app.openRides();
      await until(() => !app.view().checking, 15_000);
      await panel.resolve(ride.id, "cloud");
      const settled = await until(synced, 15_000);
      note(
        "板で「クラウドで上書き」を選ぶ",
        "開いている走行が端末 A の題に変わり、「同期済み」に戻る",
        settled !== null && app.doc()?.ride.title === `${MARK} A4` && (await diskTitle(ride.id)) === `${MARK} A4`,
        { screen: app.doc()?.ride.title, disk: await diskTitle(ride.id), message: panel.snapshot().message, shown: shown() },
      );
      app.closeRides();
      // 次の段のために、送れないまま手元で直しておく。
      await wire(DEAD);
      await app.setTitle(`${MARK} B4`);
      const offline = await until(() => app.bar() === "unsynced", 15_000);
      note("通信できない間に、端末 B で題を直す", "「未同期」になる", offline !== null, { shown: shown() });
    },

    /** 端末 A。端末 B が送れずにいる間に、同じ走行を直して送る。 */
    "a-diverge": async () => {
      await wire(apiUrl);
      await startUp();
      const ride = await openMine();
      const sent = await retitle(`${MARK} A5`);
      note("端末 A でも題を直す", "自動で送られる", sent !== null && (await cloudTitle(ride.id)) === `${MARK} A5`, { waitedMs: sent });
    },

    /** 端末 B。起動のときの突き合わせで、両方で変わっていると分かる。 */
    "b-startup-conflict": async () => {
      await wire(apiUrl);
      await startUp();
      const ride = await mine();
      const id = ride?.id ?? "";
      note(
        "両方で直した走行がある状態で、端末 B を起動する",
        "どちらでも上書きせず、「両方で変更」になり、知らせが 1 度出る",
        app.bar() === "conflict" && shown() === "両方で変更" && (await diskTitle(id)) === `${MARK} B4` && (await cloudTitle(id)) === `${MARK} A5` && app.notices().length === 1,
        { shown: shown(), disk: await diskTitle(id), cloud: await cloudTitle(id), notices: app.notices() },
      );
      const panel = await app.openRides();
      await until(() => !app.view().checking, 15_000);
      await panel.resolve(id, "local");
      const settled = await until(synced, 15_000);
      note("板で「手元で上書き」を選ぶ", "クラウドが端末 B の題になり、「同期済み」に戻る", settled !== null && (await cloudTitle(id)) === `${MARK} B4`, {
        cloud: await cloudTitle(id),
        notices: app.notices(),
      });
      app.closeRides();
    },

    /** 端末 A。走行を消すと、クラウドからも消えること。通信できないときは、消さずに理由を出すこと。 */
    "a-delete": async () => {
      await wire(apiUrl);
      await startUp();
      const ride = await openMine();
      const panel = await app.openRides();
      await until(() => !app.view().checking, 15_000);
      // ごみ箱のボタンを押して、確かめる文言を出す。
      document.querySelector<HTMLButtonElement>(`dialog button[aria-label="${ride.ride_date} の記録を消す"]`)?.click();
      await sleep(300);
      const asking = document.querySelector("dialog .asking")?.textContent?.replace(/\s+/g, " ").trim() ?? "";
      await capture("delete-ask");
      note("ごみ箱のボタンを押す", "「同期先からも削除します」と「動画ファイルは残ります」が出る。「同期先からも消す」の選択は無い", asking.includes("同期先からも削除します") && asking.includes("動画ファイルは残ります") && document.querySelector("dialog .asking input") === null, {
        asking,
      });

      await wire(DEAD);
      await panel.remove(ride.id);
      await sleep(300);
      const refused = panel.snapshot().message;
      await capture("delete-refused");
      await wire(apiUrl);
      note(
        "通信できないときに消そうとする",
        "消さずに理由を出す。手元にもクラウドにも、走行が残る",
        refused.includes("同期先から削除できない") && (await local(ride.id)) && app.doc()?.ride.id === ride.id && (await cloudTitle(ride.id)) !== null,
        { message: refused, local: await local(ride.id), open: app.doc()?.ride.id === ride.id, cloud: await cloudTitle(ride.id) },
      );

      await panel.remove(ride.id);
      const rows: SyncRow[] = await invoke("sync_list");
      note(
        "繋がるときに消す",
        "手元からもクラウドからも消え、開いていた走行は閉じる",
        !(await local(ride.id)) && (await cloudTitle(ride.id)) === null && !rows.some((row) => row.id === ride.id) && app.doc() === null && panel.snapshot().message === "",
        { local: await local(ride.id), cloud: await cloudTitle(ride.id), open: app.doc()?.ride.id ?? null, message: panel.snapshot().message, told: app.message() },
      );
      app.closeRides();

      // 1 度も送っていない走行は、クラウドに無いので、通信できなくても消せる。
      await wire(DEAD);
      await app.create("2031-02-01");
      const fresh = app.doc()?.ride.id ?? "";
      await until(() => app.bar() === "unsynced", 15_000);
      const again = await app.openRides();
      await again.remove(fresh);
      note("1 度も送っていない走行を、通信できないときに消す", "消せる", fresh !== "" && !(await local(fresh)) && again.snapshot().message === "", {
        local: await local(fresh),
        message: again.snapshot().message,
      });
      app.closeRides();
      await wire(apiUrl);
      await startUp();
      note("繋がるようにして、突き合わせる", "消した走行は、どちらもクラウドに無い。「同期済み」になる", synced() && (await cloudTitle(fresh)) === null && (await cloudTitle(ride.id)) === null, {
        shown: shown(),
      });
    },

    /**
     * 端末 B。端末 A が消した走行が、端末 B の手元に残っている。
     *
     * いまの決まりでは、手元にしか無い走行として送り直すので、クラウドに戻る。ここでは、そう
     * なることを記録するだけで、良し悪しは決めない。最後に、確認で作った走行を消して片付ける。
     */
    "b-after-delete": async () => {
      await wire(apiUrl);
      const ride = await mine();
      const id = ride?.id ?? "";
      const before = await cloudTitle(id);
      await startUp();
      const after = await cloudTitle(id);
      steps.push({
        step: "端末 A が消した走行を持ったまま、端末 B を起動する",
        observed: "手元にしか無い走行として送り直すので、クラウドに戻る",
        cloudBefore: before,
        cloudAfter: after,
        local: await local(id),
        shown: shown(),
      });
      const panel = await app.openRides();
      await until(() => !app.view().checking, 15_000);
      await panel.remove(id);
      note("端末 B でも消す", "手元からもクラウドからも消える", !(await local(id)) && (await cloudTitle(id)) === null, {
        message: panel.snapshot().message,
      });
      app.closeRides();
    },
  };

  const run = STEPS[step];
  if (run === undefined) {
    return { error: `ROUTELOG_CHECK_STEP に段の名前を入れてください: ${Object.keys(STEPS).join(", ")}`, step };
  }
  await run();
  const checked = steps.filter((entry) => "ok" in entry);
  return {
    step,
    passed: checked.filter((entry) => entry.ok === true).length,
    failed: checked.filter((entry) => entry.ok !== true).map((entry) => entry.step),
    steps,
    captures,
  };
}
