/**
 * クラウドとの自動の同期。いつ送り、いつ受け取るかを決める。
 *
 * 画面にも Tauri にも触らない。通信は `Backend`、画面への頼みごとは `Host` を通すので、単体で試せる。
 */

import type { RideDocument } from "./types";
import type { SaveState, SyncRow } from "./ui";

/** 保存のあと、送るまで待つ時間。待つ間に次の保存が来たら、待ち直す */
export const PUSH_DELAY_MS = 3000;
/** 通信できなかったあと、もう一度試すまで待つ時間 */
export const RETRY_DELAY_MS = 60_000;

/**
 * 走行 1 つの、クラウドとの同期の状態。
 *
 * - `synced`: 手元とクラウドが同じ
 * - `waiting`: 保存したので、少し置いてから送る
 * - `sending`: 送っているか、受け取っている
 * - `unsent`: 通信できなかった。あとでもう一度試す
 * - `conflict`: 両方で変わった。どちらを残すかを人が選ぶ
 */
export type RideSync = "synced" | "waiting" | "sending" | "unsent" | "conflict";

/** 画面に出す、同期の全体の状態。 */
export interface SyncView {
  /** 同期の接続先を設定してあるか */
  configured: boolean;
  /** クラウドと突き合わせている最中か */
  checking: boolean;
  /** 走行ごとの状態。まだ突き合わせていない走行は入っていない */
  rides: Record<string, RideSync>;
  /** クラウドにだけあって、まだ受け取れていない走行 */
  remote: SyncRow[];
  /** 通信できなかった理由。通信できていれば空 */
  failure: string;
}

export const NO_SYNC: SyncView = { configured: false, checking: false, rides: {}, remote: [], failure: "" };

/** 上の帯に 1 つだけ出す、保存と同期の状態。 */
export type BarState = "saved" | "saving" | "save_error" | "syncing" | "synced" | "unsynced" | "conflict";

/**
 * 保存の状態と同期の状態から、上の帯に出す 1 つを選ぶ。出すものが無ければ null。
 *
 * 手元への保存を先に見る。同期を設定していなければ、保存のことだけを出す。走行を開いて
 * いなくても、同期を設定していれば同期の状態を出す。
 */
export function barState(save: SaveState | null, view: SyncView): BarState | null {
  if (save === "error") return "save_error";
  if (save === "pending" || save === "saving") return "saving";
  if (!view.configured) return save === null ? null : "saved";
  const rides = Object.values(view.rides);
  if (view.checking || rides.includes("waiting") || rides.includes("sending")) return "syncing";
  if (rides.includes("conflict")) return "conflict";
  if (view.failure !== "" || rides.includes("unsent")) return "unsynced";
  return "synced";
}

/** Rust 側の同期のコマンド。 */
export interface Backend {
  /** 同期の接続先を設定してあるか */
  configured(): Promise<boolean>;
  /** 手元とクラウドの走行を突き合わせた一覧 */
  list(): Promise<SyncRow[]>;
  /** 手元の走行を送る。クラウドが別の場所で書き換えられていたら、送らずに `conflict` を返す */
  push(id: string, overwrite: boolean): Promise<"sent" | "conflict">;
  /** クラウドの走行を取ってくる。手元には置かない */
  fetch(id: string): Promise<RideDocument>;
  /** クラウドの走行を消す */
  remove(id: string): Promise<void>;
}

/** 画面の側に頼むこと。 */
export interface Host {
  /** 待っている編集を、手元に保存し終える */
  settle(): Promise<void>;
  /**
   * 取ってきた走行を手元に置く。置けたら true。
   *
   * 手元の走行の更新時刻が `expect` と違うか、保存できていない編集があれば、置かずに false を返す。
   * `overwrite` なら、確かめずに置く。
   */
  apply(doc: RideDocument, expect: string | null, overwrite: boolean): Promise<boolean>;
  /** 同期の状態が変わった */
  show(view: SyncView): void;
  /** 同期できなかった。同期できるようになるまでに 1 度だけ呼ぶ */
  offline(reason: string): void;
  /** 両方で変わった走行が見つかった。走行ごとに 1 度だけ呼ぶ */
  conflicted(id: string): void;
}

/** 時間を置いて呼ぶ口。試すときに差し替える。 */
export interface Timers {
  set(run: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}

const CLOCK: Timers = {
  set: (run, ms) => setTimeout(run, ms),
  clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

export class CloudSync {
  private readonly backend: Backend;
  private readonly host: Host;
  private readonly timers: Timers;
  private view: SyncView = NO_SYNC;
  /** 送るのを待っている走行と、その待ち時間 */
  private waiting = new Map<string, unknown>();
  /** 送り受けの最中の走行。同じ走行の送り受けは、1 つずつ行う */
  private working = new Map<string, Promise<void>>();
  /** 送り受けの間に、送るよう頼まれた走行。済んだら、最新を送る */
  private again = new Set<string>();
  /** 消そうとしている走行。値は、その間に送らずに置いた保存があるか */
  private held = new Map<string, boolean>();
  /** 消そうとしている間に、受け取るのをやめた走行 */
  private skipped = new Set<string>();
  /** クラウドの一覧を頼んだあとで、事情が変わった走行。その一覧の行は、もう当てにならない */
  private touched = new Set<string>();
  /** もう一度試すまでの待ち時間 */
  private retry: unknown;
  /** 走っている突き合わせ */
  private checking: Promise<void> | undefined;
  /** 突き合わせの間に、もう一度頼まれたか */
  private recheck = false;
  /** クラウドの一覧を取れないままか */
  private stale = false;
  /** 同期できないことを、もう知らせたか */
  private told = false;

  constructor(backend: Backend, host: Host, timers: Timers = CLOCK) {
    this.backend = backend;
    this.host = host;
    this.timers = timers;
  }

  /** いまの状態。 */
  get current(): SyncView {
    return this.view;
  }

  /**
   * 接続先を読み直し、設定してあればクラウドと突き合わせる。
   *
   * 起動のときと、接続先を保存したあとに呼ぶ。接続先が変わると、前の状態は当てにならないので捨てる。
   */
  async start(): Promise<void> {
    const configured = await this.backend.configured();
    for (const id of [...this.waiting.keys()]) this.cancel(id);
    this.clearRetry();
    this.stale = false;
    this.told = false;
    this.publish({ ...this.view, configured, rides: {}, remote: [], failure: "" });
    if (configured) await this.check();
  }

  /**
   * クラウドと突き合わせ、迷わないものを送り受けする。両方で変わったものは、人に聞く。
   *
   * 走っている間に頼まれたら、済んでからもう 1 度だけ突き合わせる。
   */
  check(): Promise<void> {
    if (this.checking !== undefined) {
      this.recheck = true;
      return this.checking;
    }
    if (!this.view.configured) return Promise.resolve();
    const rounds = async () => {
      this.publish({ ...this.view, checking: true });
      try {
        do {
          this.recheck = false;
          await this.reconcile();
        } while (this.recheck);
      } finally {
        this.checking = undefined;
        this.publish({ ...this.view, checking: false });
      }
    };
    this.checking = rounds();
    return this.checking;
  }

  /**
   * 走行を手元に保存した。少し置いてから送る。待つ間に次の保存が来たら、待ち直す。
   *
   * 続けて編集している間は、保存が続くので送らない。
   */
  saved(id: string): void {
    if (!this.view.configured) return;
    this.touched.add(id);
    if (this.held.has(id)) {
      this.held.set(id, true);
      return;
    }
    if (this.view.rides[id] === "conflict") {
      // 人が選ぶまでは送らない。選んだほうで揃えている最中なら、済んでから送る。
      if (this.working.has(id)) this.again.add(id);
      return;
    }
    this.cancel(id);
    const fire = () => {
      this.waiting.delete(id);
      void this.push(id);
    };
    this.waiting.set(id, this.timers.set(fire, PUSH_DELAY_MS));
    if (!this.working.has(id)) this.mark(id, "waiting");
  }

  /**
   * 両方で変わった走行を、選んだほうで揃える。
   *
   * 失敗したら投げる。走行は「両方で変更」のまま残るので、選び直せる。
   */
  async resolve(id: string, keep: "local" | "cloud"): Promise<void> {
    this.cancel(id);
    await this.idle(id);
    const failures: unknown[] = [];
    await this.run(id, async () => {
      try {
        if (keep === "local") {
          await this.host.settle();
          // ここまでの保存は、次の送信に入る。
          this.again.delete(id);
          const outcome = await this.backend.push(id, true);
          if (outcome === "conflict") throw new Error("同期先の走行が再び更新されました。もう一度選択してください");
        } else {
          const doc = await this.backend.fetch(id);
          await this.host.apply(doc, null, true);
        }
        this.settled(id);
      } catch (e) {
        failures.push(e);
      }
    });
    if (failures.length > 0) throw failures[0];
    this.reached();
  }

  /**
   * 走行をクラウドから消す。消せなければ投げる。同期を設定していなければ、何もしない。
   *
   * 呼んだら、その走行はもう送らず、受け取らない。送り受けの最中なら、済むのを待ってから消す。
   * 手元の走行を消し終えたら、`forget` を呼ぶ。
   */
  async remove(id: string): Promise<void> {
    this.touched.add(id);
    this.held.set(id, this.waiting.has(id));
    this.cancel(id);
    await this.idle(id);
    try {
      if (this.view.configured) await this.backend.remove(id);
    } catch (e) {
      // 消すのをやめる。その間に送らずに置いた保存があれば送り直し、受け取るのをやめていたら突き合わせ直す。
      const unsent = this.held.get(id);
      this.held.delete(id);
      if (unsent) this.saved(id);
      if (this.skipped.delete(id)) void this.check();
      throw e;
    }
  }

  /** 手元の走行を消し終えた。その走行のことを忘れる。 */
  forget(id: string): void {
    this.touched.add(id);
    this.held.delete(id);
    this.again.delete(id);
    this.skipped.delete(id);
    this.publish({ ...this.view, rides: this.without(id), remote: this.view.remote.filter((row) => row.id !== id) });
    this.recover();
  }

  private async reconcile(): Promise<void> {
    if (!this.view.configured) return;
    this.clearRetry();
    this.touched.clear();
    let rows: SyncRow[];
    try {
      rows = await this.backend.list();
    } catch (e) {
      this.stale = true;
      this.fail(e);
      return;
    }
    this.stale = false;
    // どちらにも無くなった走行の状態を捨てる。保存したばかりで、まだ一覧に無い走行は残す。
    const listed = new Set(rows.map((row) => row.id));
    const rides = Object.fromEntries(Object.entries(this.view.rides).filter(([id]) => listed.has(id) || this.busy(id)));
    this.publish({ ...this.view, rides, remote: rows.filter((row) => row.local_updated_at === null) });
    for (const row of rows) {
      // 突き合わせの間に、接続先が消された。
      if (!this.view.configured) return;
      // 送るのを待っている走行は、待ち時間に任せる。編集が続いている間は送らない。
      if (this.held.has(row.id) || this.busy(row.id)) continue;
      // 一覧を頼んだあとで、送った・失敗した・人が揃えた・消した。古い行のとおりにはせず、取り直す。
      if (this.touched.has(row.id)) {
        this.recheck = true;
        continue;
      }
      if (row.action === "push") await this.push(row.id);
      else if (row.action === "pull") await this.run(row.id, () => this.receive(row));
      else if (row.action === "ask") this.conflict(row.id);
      else this.mark(row.id, "synced");
    }
    this.recover();
  }

  /** 走行を送る。同じ走行の送り受けは重ねない。その最中に頼まれたら、済んでから最新を送る。 */
  private push(id: string): Promise<void> {
    if (this.held.has(id)) {
      this.held.set(id, true);
      return Promise.resolve();
    }
    const running = this.working.get(id);
    if (running !== undefined) {
      this.again.add(id);
      return running;
    }
    return this.run(id, () => this.send(id));
  }

  /**
   * 走行 1 つの送り受けを行う。済むまで、同じ走行の次の送り受けを始めさせない。
   *
   * `work` は投げないこと。その間に送るよう頼まれていたら、済んでから送る。
   */
  private run(id: string, work: () => Promise<void>): Promise<void> {
    const job = (async () => {
      this.touched.add(id);
      try {
        await work();
      } finally {
        this.working.delete(id);
        this.touched.add(id);
      }
      const state = this.view.rides[id];
      if (this.again.delete(id) && state !== "conflict" && state !== "unsent") await this.push(id);
    })();
    this.working.set(id, job);
    return job;
  }

  /** その走行の送り受けが済むのを待つ。 */
  private async idle(id: string): Promise<void> {
    while (this.working.has(id)) await this.working.get(id);
  }

  private async send(id: string): Promise<void> {
    this.mark(id, "sending");
    try {
      const outcome = await this.backend.push(id, false);
      if (outcome === "conflict") {
        this.conflict(id);
        return;
      }
      this.settled(id);
      this.reached();
    } catch (e) {
      this.fail(e, id);
    }
  }

  private async receive(row: SyncRow): Promise<void> {
    this.mark(row.id, "sending");
    try {
      const doc = await this.backend.fetch(row.id);
      if (this.held.has(row.id)) {
        // 取ってくる間に、消すことになった。手元に置くと、消した走行が戻ってしまう。
        this.skipped.add(row.id);
        this.publish({ ...this.view, rides: this.without(row.id) });
        return;
      }
      const placed = await this.host.apply(doc, row.local_updated_at, false);
      // 置けなかったのは、取ってくる間に手元も変わったからである。
      if (placed) this.settled(row.id);
      else this.conflict(row.id);
    } catch (e) {
      this.fail(e, row.id);
    }
  }

  /** 手元とクラウドが揃った。次の保存がもう待っていれば、同期済みにはしない。 */
  private settled(id: string): void {
    const rides = { ...this.view.rides, [id]: this.waiting.has(id) ? ("waiting" as const) : ("synced" as const) };
    this.publish({ ...this.view, rides, remote: this.view.remote.filter((row) => row.id !== id) });
  }

  /** 両方で変わった走行として扱う。人が選ぶまで、送らない。 */
  private conflict(id: string): void {
    this.touched.add(id);
    this.cancel(id);
    this.again.delete(id);
    const known = this.view.rides[id] === "conflict";
    this.mark(id, "conflict");
    if (!known) this.host.conflicted(id);
  }

  /** 同期できなかった。少し置いて、もう一度突き合わせる。 */
  private fail(e: unknown, id?: string): void {
    if (!this.view.configured) return;
    const failure = e instanceof Error ? e.message : String(e);
    let rides = this.view.rides;
    if (id !== undefined) {
      this.touched.add(id);
      rides = { ...rides, [id]: "unsent" };
    }
    this.publish({ ...this.view, rides, failure });
    if (!this.told) {
      this.told = true;
      this.host.offline(failure);
    }
    this.retryLater();
  }

  /** クラウドに届いた。同期できずに残っているものがあれば、待たずに突き合わせ直す。 */
  private reached(): void {
    if (this.checking === undefined && this.behind()) void this.check();
    else this.recover();
  }

  /**
   * 同期できずに残っているものが無くなったら、同期できなかったことを忘れる。
   *
   * 残っているなら、もう一度試す予約があることを確かめる。
   */
  private recover(): void {
    if (this.behind()) {
      this.retryLater();
      return;
    }
    this.told = false;
    this.clearRetry();
    if (this.view.failure !== "") this.publish({ ...this.view, failure: "" });
  }

  /** 同期できずに残っているものがあるか。 */
  private behind(): boolean {
    return this.stale || Object.values(this.view.rides).includes("unsent");
  }

  private busy(id: string): boolean {
    return this.waiting.has(id) || this.working.has(id);
  }

  private cancel(id: string): void {
    const timer = this.waiting.get(id);
    if (timer === undefined) return;
    this.timers.clear(timer);
    this.waiting.delete(id);
  }

  private retryLater(): void {
    this.retry ??= this.timers.set(() => {
      this.retry = undefined;
      void this.check();
    }, RETRY_DELAY_MS);
  }

  private clearRetry(): void {
    if (this.retry === undefined) return;
    this.timers.clear(this.retry);
    this.retry = undefined;
  }

  /** その走行を除いた、走行ごとの状態。 */
  private without(id: string): Record<string, RideSync> {
    const rides = { ...this.view.rides };
    delete rides[id];
    return rides;
  }

  private mark(id: string, state: RideSync): void {
    if (this.view.rides[id] === state) return;
    this.publish({ ...this.view, rides: { ...this.view.rides, [id]: state } });
  }

  private publish(view: SyncView): void {
    this.view = view;
    this.host.show(view);
  }
}
