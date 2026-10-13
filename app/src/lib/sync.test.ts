import { describe, expect, test } from "bun:test";
import { barState, CloudSync, NO_SYNC, PUSH_DELAY_MS, RETRY_DELAY_MS, type SyncView } from "./sync";
import type { RideDocument } from "./types";
import type { SyncRow } from "./ui";

/** 約束の続きを、すべて走らせる。 */
async function settle() {
  for (let i = 0; i < 50; i++) await Promise.resolve();
}

/** 手で進める時計。 */
function clock() {
  let now = 0;
  let timers: { at: number; run: () => void }[] = [];
  return {
    set(run: () => void, ms: number) {
      const timer = { at: now + ms, run };
      timers.push(timer);
      return timer;
    },
    clear(handle: unknown) {
      timers = timers.filter((timer) => timer !== handle);
    },
    /** 時間を進め、その間に来た待ち時間を、早い順に呼ぶ。 */
    async advance(ms: number) {
      const until = now + ms;
      for (;;) {
        const next = timers.filter((timer) => timer.at <= until).sort((a, b) => a.at - b.at)[0];
        if (next === undefined) break;
        timers = timers.filter((timer) => timer !== next);
        now = next.at;
        next.run();
        await settle();
      }
      now = until;
      await settle();
    },
  };
}

/** 手元とクラウドの代わり。走行は、ID と更新時刻だけで表す。 */
function world() {
  const local = new Map<string, string>();
  const remote = new Map<string, string>();
  const synced = new Map<string, string>();
  const time = clock();
  const log: string[] = [];
  const views: SyncView[] = [];
  const state = {
    online: true,
    /** 走行を取ってこられるか。一覧は取れても、走行を取る所で失敗する場合の代わり */
    fetchable: true,
    configured: true,
    /** 送れるか。一覧は取れても、送る所で失敗する場合の代わり */
    pushable: true,
    /** 送信を止めておく栓。抜くまで、送信は終わらない */
    gate: undefined as Promise<void> | undefined,
    /** 走行を取ってくるのを止めておく栓 */
    fetchGate: undefined as Promise<void> | undefined,
    /** クラウドから消せるか */
    removable: true,
    /** クラウドから消すのを止めておく栓 */
    removeGate: undefined as Promise<void> | undefined,
    /** 手元に置くのを断るか。取ってくる間に手元が変わったときの代わり */
    refuse: false,
    sendingNow: 0,
    sentTogether: 0,
  };
  const reach = () => {
    if (!state.online) throw "クラウドに繋がりません";
  };
  const row = (id: string): SyncRow => {
    const [here, there, known] = [local.get(id), remote.get(id), synced.get(id)];
    const state =
      there === undefined
        ? "local_only"
        : here === undefined
          ? "remote_only"
          : here === there
            ? "same"
            : known === there
              ? "local_newer"
              : known === here
                ? "remote_newer"
                : "conflict";
    const action = (
      { same: "keep", local_newer: "push", local_only: "push", remote_newer: "pull", remote_only: "pull", conflict: "ask" } as const
    )[state];
    return { id, title: id, ride_date: "2026-10-03", local_updated_at: here ?? null, remote_updated_at: there ?? null, state, action };
  };
  const sync = new CloudSync(
    {
      configured: async () => state.configured,
      list: async () => {
        reach();
        log.push("list");
        return [...new Set([...local.keys(), ...remote.keys()])].sort().map(row);
      },
      push: async (id, overwrite) => {
        const sending = local.get(id) ?? "";
        state.sendingNow += 1;
        state.sentTogether = Math.max(state.sentTogether, state.sendingNow);
        try {
          await state.gate;
          reach();
          if (!state.pushable) throw "クラウドが 500 を返しました";
          if (remote.has(id) && remote.get(id) !== synced.get(id) && !overwrite) return "conflict";
          remote.set(id, sending);
          synced.set(id, sending);
          log.push(`push ${id} ${sending}`);
          return "sent";
        } finally {
          state.sendingNow -= 1;
        }
      },
      fetch: async (id) => {
        reach();
        if (!state.fetchable) throw "クラウドが 500 を返しました";
        const doc = { ride: { id, updated_at: remote.get(id) } } as RideDocument;
        await state.fetchGate;
        return doc;
      },
      remove: async (id) => {
        await state.removeGate;
        reach();
        if (!state.removable) throw "クラウドが 500 を返しました";
        remote.delete(id);
        synced.delete(id);
        log.push(`remove ${id}`);
      },
    },
    {
      settle: async () => void log.push("settle"),
      apply: async (doc, expect, overwrite) => {
        if (state.refuse || (!overwrite && (local.get(doc.ride.id) ?? null) !== expect)) return false;
        local.set(doc.ride.id, doc.ride.updated_at);
        synced.set(doc.ride.id, doc.ride.updated_at);
        log.push(`apply ${doc.ride.id} ${doc.ride.updated_at}`);
        return true;
      },
      show: (view) => views.push(view),
      offline: (reason) => void log.push(`offline: ${reason}`),
      conflicted: (id) => void log.push(`conflicted ${id}`),
    },
    time,
  );
  /** 手元で保存する。 */
  const save = (id: string, stamp: string) => {
    local.set(id, stamp);
    sync.saved(id);
  };
  /** 栓をする。返した関数を呼ぶと抜ける。 */
  const plug = (which: "gate" | "fetchGate" | "removeGate") => {
    let open = () => {};
    state[which] = new Promise((resolve) => (open = resolve));
    return () => {
      state[which] = undefined;
      open();
    };
  };
  return { sync, local, remote, synced, time, log, views, state, save, plug, bar: () => barState("saved", sync.current) };
}

describe("上の帯に出す状態", () => {
  const view = (rides: SyncView["rides"], more: Partial<SyncView> = {}): SyncView => ({
    ...NO_SYNC,
    configured: true,
    rides,
    ...more,
  });

  test("同期を設定していなければ、保存のことだけを出す", () => {
    expect(barState("saved", NO_SYNC)).toBe("saved");
    expect(barState("pending", NO_SYNC)).toBe("saving");
    expect(barState("saving", NO_SYNC)).toBe("saving");
    expect(barState("error", NO_SYNC)).toBe("save_error");
    expect(barState(null, NO_SYNC)).toBeNull();
  });

  test("手元への保存を、同期より先に出す", () => {
    const conflicted = view({ a: "conflict" });
    expect(barState("pending", conflicted)).toBe("saving");
    expect(barState("saving", conflicted)).toBe("saving");
    expect(barState("error", conflicted)).toBe("save_error");
  });

  test("保存が済んでいれば、同期の状態を 1 つ出す", () => {
    expect(barState("saved", view({ a: "synced", b: "synced" }))).toBe("synced");
    expect(barState("saved", view({}))).toBe("synced");
    expect(barState("saved", view({ a: "synced", b: "waiting" }))).toBe("syncing");
    expect(barState("saved", view({ a: "sending" }))).toBe("syncing");
    expect(barState("saved", view({ a: "synced" }, { checking: true }))).toBe("syncing");
    expect(barState("saved", view({ a: "synced", b: "unsent" }))).toBe("unsynced");
    expect(barState("saved", view({ a: "synced" }, { failure: "クラウドに繋がりません" }))).toBe("unsynced");
    expect(barState("saved", view({ a: "unsent", b: "conflict" }))).toBe("conflict");
    expect(barState("saved", view({ a: "sending", b: "conflict" }))).toBe("syncing");
  });

  test("走行を開いていなくても、同期の状態は出す", () => {
    expect(barState(null, view({ a: "conflict" }))).toBe("conflict");
    expect(barState(null, view({}))).toBe("synced");
  });
});

describe("保存のあとの送信", () => {
  test("少し置いてから送り、同期済みになる", async () => {
    const w = world();
    await w.sync.start();
    w.save("a", "t1");
    expect(w.bar()).toBe("syncing");
    await w.time.advance(PUSH_DELAY_MS - 1);
    expect(w.remote.has("a")).toBe(false);
    await w.time.advance(1);
    expect(w.remote.get("a")).toBe("t1");
    expect(w.sync.current.rides).toEqual({ a: "synced" });
    expect(w.bar()).toBe("synced");
  });

  test("続けて保存している間は送らず、止めてから 1 度だけ送る", async () => {
    const w = world();
    await w.sync.start();
    for (const stamp of ["t1", "t2", "t3", "t4"]) {
      w.save("a", stamp);
      await w.time.advance(PUSH_DELAY_MS - 500);
      expect(w.remote.has("a")).toBe(false);
    }
    await w.time.advance(500);
    expect(w.log.filter((line) => line.startsWith("push"))).toEqual(["push a t4"]);
  });

  test("送っている間に次の保存が来たら、重ねずに、済んでから最新を送る", async () => {
    const w = world();
    await w.sync.start();
    let open = () => {};
    w.state.gate = new Promise((resolve) => (open = resolve));
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.state.sendingNow).toBe(1);
    // 送っている最中に 2 度保存する。待ち時間が過ぎても、前の送信が済むまでは送らない。
    w.save("a", "t2");
    w.save("a", "t3");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.state.sendingNow).toBe(1);
    expect(w.sync.current.rides.a).toBe("sending");
    w.state.gate = undefined;
    open();
    await settle();
    expect(w.state.sentTogether).toBe(1);
    expect(w.remote.get("a")).toBe("t3");
    expect(w.sync.current.rides.a).toBe("synced");
  });

  test("送り終えたときに次の保存が待っていれば、同期済みにしない", async () => {
    const w = world();
    await w.sync.start();
    let open = () => {};
    w.state.gate = new Promise((resolve) => (open = resolve));
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    w.save("a", "t2");
    w.state.gate = undefined;
    open();
    await settle();
    expect(w.sync.current.rides.a).toBe("waiting");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.remote.get("a")).toBe("t2");
    expect(w.sync.current.rides.a).toBe("synced");
  });

  test("接続先を消したら、待っていた送信をやめる", async () => {
    const w = world();
    await w.sync.start();
    w.save("a", "t1");
    w.state.configured = false;
    await w.sync.start();
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.remote.has("a")).toBe(false);
    expect(w.sync.current.rides).toEqual({});
    expect(w.bar()).toBe("saved");
  });

  test("同期を設定していなければ、何も送らない", async () => {
    const w = world();
    w.state.configured = false;
    await w.sync.start();
    w.save("a", "t1");
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.log).toEqual([]);
    expect(w.bar()).toBe("saved");
  });
});

describe("クラウドとの突き合わせ", () => {
  test("新しいほうへ揃え、片方にしか無い走行は他方へ渡す", async () => {
    const w = world();
    const set = (id: string, here?: string, there?: string, known?: string) => {
      if (here) w.local.set(id, here);
      if (there) w.remote.set(id, there);
      if (known) w.synced.set(id, known);
    };
    set("same", "t1", "t1", "t1");
    set("edited-here", "t2", "t1", "t1");
    set("edited-there", "t1", "t2", "t1");
    set("new-here", "t1");
    set("new-there", undefined, "t1");
    await w.sync.start();
    expect(w.log).toEqual(["list", "push edited-here t2", "apply edited-there t2", "push new-here t1", "apply new-there t1"]);
    expect(Object.fromEntries(w.local)).toEqual(Object.fromEntries(w.remote));
    expect(w.local.get("edited-here")).toBe("t2");
    expect(w.local.get("edited-there")).toBe("t2");
    expect(new Set(Object.values(w.sync.current.rides))).toEqual(new Set(["synced"]));
    expect(w.sync.current.remote).toEqual([]);
    expect(w.bar()).toBe("synced");
  });

  test("両方で変わった走行は、どちらでも上書きせず、1 度だけ知らせる", async () => {
    const w = world();
    w.local.set("a", "t2");
    w.remote.set("a", "t3");
    w.synced.set("a", "t1");
    await w.sync.start();
    expect(w.sync.current.rides.a).toBe("conflict");
    expect(w.bar()).toBe("conflict");
    expect([w.local.get("a"), w.remote.get("a")]).toEqual(["t2", "t3"]);
    await w.sync.check();
    await w.sync.check();
    expect(w.log.filter((line) => line.startsWith("conflicted"))).toEqual(["conflicted a"]);
  });

  test("取ってくる間に手元も変わった走行は、置かずに「両方で変更」にする", async () => {
    const w = world();
    w.local.set("a", "t1");
    w.remote.set("a", "t2");
    w.synced.set("a", "t1");
    w.state.refuse = true;
    await w.sync.start();
    expect(w.local.get("a")).toBe("t1");
    expect(w.sync.current.rides.a).toBe("conflict");
    expect(w.log).toContain("conflicted a");
  });

  test("送るのを待っている走行は、突き合わせでは送らない", async () => {
    const w = world();
    await w.sync.start();
    w.save("a", "t1");
    await w.sync.check();
    expect(w.remote.has("a")).toBe(false);
    expect(w.sync.current.rides.a).toBe("waiting");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.remote.get("a")).toBe("t1");
  });

  test("突き合わせの間に頼まれたら、済んでからもう 1 度だけ突き合わせる", async () => {
    const w = world();
    w.local.set("a", "t1");
    await w.sync.start();
    const first = w.sync.check();
    void w.sync.check();
    void w.sync.check();
    await first;
    // 起動のときの 1 度と、頼まれた 1 度と、その間に頼まれた分の 1 度である。
    expect(w.log.filter((line) => line === "list").length).toBe(3);
    expect(w.sync.current.checking).toBe(false);
  });
});

describe("突き合わせの間に事情が変わった走行", () => {
  /** 走行 a を受け取っている途中で止まった所まで進める。x は、手元とクラウドで同じである。 */
  async function receiving(prepare: (w: ReturnType<typeof world>) => void = () => {}) {
    const w = world();
    w.local.set("x", "t1");
    w.remote.set("x", "t1");
    w.synced.set("x", "t1");
    prepare(w);
    await w.sync.start();
    w.remote.set("a", "t1");
    const open = w.plug("fetchGate");
    const checking = w.sync.check();
    await settle();
    return { w, open, checking };
  }

  test("その間に送れなかった走行を、古い一覧のとおりに「同期済み」にしない", async () => {
    const { w, open, checking } = await receiving();
    w.state.pushable = false;
    w.save("x", "t2");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.sync.current.rides.x).toBe("unsent");
    open();
    await checking;
    expect(w.sync.current.rides.x).toBe("unsent");
    expect(w.bar()).toBe("unsynced");
    // もう一度試す予約も残っている。
    w.state.pushable = true;
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.remote.get("x")).toBe("t2");
    expect(w.bar()).toBe("synced");
  });

  test("その間に人が揃えた走行を、古い一覧のとおりに「両方で変更」へ戻さない", async () => {
    const { w, open, checking } = await receiving((w) => {
      w.local.set("x", "t2");
      w.remote.set("x", "t3");
    });
    expect(w.sync.current.rides.x).toBe("conflict");
    await w.sync.resolve("x", "local");
    open();
    await checking;
    expect(w.remote.get("x")).toBe("t2");
    expect(w.sync.current.rides.x).toBe("synced");
    expect(w.log.filter((line) => line.startsWith("conflicted"))).toEqual(["conflicted x"]);
  });

  test("その間に消した走行を、古い一覧のとおりに送ろうとしない", async () => {
    const { w, open, checking } = await receiving((w) => w.local.set("y", "t1"));
    // y は起動のときに送ってある。手元で直してから消す。
    w.local.set("y", "t2");
    await w.sync.remove("y");
    w.local.delete("y");
    w.sync.forget("y");
    open();
    await checking;
    expect(w.remote.has("y")).toBe(false);
    expect(w.sync.current.rides).toEqual({ a: "synced", x: "synced" });
    expect(w.log.filter((line) => line.startsWith("offline"))).toEqual([]);
  });

  test("受け取っている途中の走行を消すと、取ってきた内容を手元に置かない", async () => {
    const { w, open, checking } = await receiving();
    const removing = w.sync.remove("a");
    await settle();
    // 受け取りが済むまで、クラウドから消すのを待つ。
    expect(w.log).not.toContain("remove a");
    open();
    await removing;
    w.sync.forget("a");
    await checking;
    expect(w.local.has("a")).toBe(false);
    expect(w.remote.has("a")).toBe(false);
    expect(w.sync.current.rides).toEqual({ x: "synced" });
    expect(w.sync.current.remote).toEqual([]);
  });

  test("受け取っている途中の走行を消せなかったら、突き合わせ直して受け取る", async () => {
    const { w, open, checking } = await receiving();
    const removing = w.sync.remove("a");
    await settle();
    // 走行は取ってこられるが、消す所で失敗する。
    w.state.removable = false;
    open();
    await expect(removing).rejects.toBe("クラウドが 500 を返しました");
    await checking;
    await settle();
    expect(w.local.get("a")).toBe("t1");
    expect(w.sync.current.rides.a).toBe("synced");
  });
});

describe("通信できないとき", () => {
  test("未同期になり、知らせは 1 度だけ出す", async () => {
    const w = world();
    w.state.online = false;
    await w.sync.start();
    expect(w.bar()).toBe("unsynced");
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    w.save("a", "t2");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.sync.current.rides.a).toBe("unsent");
    expect(w.sync.current.failure).toBe("クラウドに繋がりません");
    expect(w.bar()).toBe("unsynced");
    expect(w.log.filter((line) => line.startsWith("offline"))).toEqual(["offline: クラウドに繋がりません"]);
  });

  test("少し置いてからもう一度試し、繋がれば送る", async () => {
    const w = world();
    await w.sync.start();
    w.state.online = false;
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.bar()).toBe("unsynced");
    // 繋がらないままなら、試しても未同期のままである。
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.bar()).toBe("unsynced");
    w.state.online = true;
    await w.time.advance(RETRY_DELAY_MS - 1);
    expect(w.remote.has("a")).toBe(false);
    await w.time.advance(1);
    expect(w.remote.get("a")).toBe("t1");
    expect(w.sync.current.failure).toBe("");
    expect(w.bar()).toBe("synced");
  });

  test("次の保存で繋がれば、送れずに残っていた走行も送る", async () => {
    const w = world();
    await w.sync.start();
    w.state.online = false;
    w.save("a", "t1");
    w.save("b", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.sync.current.rides).toEqual({ a: "unsent", b: "unsent" });
    w.state.online = true;
    w.save("b", "t2");
    await w.time.advance(PUSH_DELAY_MS);
    expect([w.remote.get("a"), w.remote.get("b")]).toEqual(["t1", "t2"]);
    expect(w.bar()).toBe("synced");
  });

  test("繋がり直したあとで、また繋がらなくなったら、もう一度知らせる", async () => {
    const w = world();
    await w.sync.start();
    for (const stamp of ["t1", "t2"]) {
      w.state.online = false;
      w.save("a", stamp);
      await w.time.advance(PUSH_DELAY_MS);
      w.state.online = true;
      await w.time.advance(RETRY_DELAY_MS);
      expect(w.bar()).toBe("synced");
    }
    expect(w.log.filter((line) => line.startsWith("offline")).length).toBe(2);
  });

  test("受け取れなかった走行は、クラウドにだけある走行として残り、あとで受け取る", async () => {
    const w = world();
    w.remote.set("a", "t1");
    await w.sync.start();
    expect(w.local.get("a")).toBe("t1");
    // 一覧は取れたが、走行は取ってこられない。
    w.remote.set("b", "t1");
    w.state.fetchable = false;
    await w.sync.check();
    expect(w.sync.current.remote.map((row) => row.id)).toEqual(["b"]);
    expect(w.sync.current.rides.b).toBe("unsent");
    expect(w.bar()).toBe("unsynced");
    w.state.fetchable = true;
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.local.get("b")).toBe("t1");
    expect(w.sync.current.remote).toEqual([]);
  });
});

describe("両方で変わった走行", () => {
  /** 送ろうとして、クラウドが別の場所で書き換えられていたと分かった所まで進める。 */
  async function refused() {
    const w = world();
    w.local.set("a", "t1");
    w.remote.set("a", "t1");
    w.synced.set("a", "t1");
    await w.sync.start();
    w.remote.set("a", "t9");
    w.save("a", "t2");
    await w.time.advance(PUSH_DELAY_MS);
    return w;
  }

  test("送ろうとして分かったときも、上書きせずに知らせる", async () => {
    const w = await refused();
    expect(w.remote.get("a")).toBe("t9");
    expect(w.sync.current.rides.a).toBe("conflict");
    expect(w.log.filter((line) => line.startsWith("conflicted"))).toEqual(["conflicted a"]);
  });

  test("人が選ぶまで、保存しても送らない", async () => {
    const w = await refused();
    w.save("a", "t3");
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.remote.get("a")).toBe("t9");
    expect(w.sync.current.rides.a).toBe("conflict");
  });

  test("手元を選ぶと、待っている編集を保存してから、手元の内容で置き換える", async () => {
    const w = await refused();
    await w.sync.resolve("a", "local");
    expect(w.log.slice(-2)).toEqual(["settle", "push a t2"]);
    expect(w.remote.get("a")).toBe("t2");
    expect(w.sync.current.rides.a).toBe("synced");
    expect(w.bar()).toBe("synced");
  });

  test("手元で揃えている間に来た保存は、済んでから送る", async () => {
    const w = await refused();
    const open = w.plug("gate");
    const resolving = w.sync.resolve("a", "local");
    await settle();
    w.save("a", "t3");
    open();
    await resolving;
    await settle();
    expect(w.state.sentTogether).toBe(1);
    expect(w.remote.get("a")).toBe("t3");
    expect(w.sync.current.rides.a).toBe("synced");
  });

  test("揃えている最中に消すと、揃え終わるのを待ってから消す", async () => {
    const w = await refused();
    const open = w.plug("gate");
    const resolving = w.sync.resolve("a", "local");
    await settle();
    const removing = w.sync.remove("a");
    await settle();
    expect(w.log).not.toContain("remove a");
    open();
    await resolving;
    await removing;
    expect(w.log.slice(-2)).toEqual(["push a t2", "remove a"]);
    expect(w.remote.has("a")).toBe(false);
  });

  test("クラウドを選ぶと、クラウドの内容を手元に置く", async () => {
    const w = await refused();
    await w.sync.resolve("a", "cloud");
    expect(w.local.get("a")).toBe("t9");
    expect(w.sync.current.rides.a).toBe("synced");
    // 揃えたあとは、また自動で送る。
    w.save("a", "t10");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.remote.get("a")).toBe("t10");
  });

  test("選んだときに繋がらなければ、「両方で変更」のまま残る", async () => {
    const w = await refused();
    w.state.online = false;
    await expect(w.sync.resolve("a", "cloud")).rejects.toBe("クラウドに繋がりません");
    await expect(w.sync.resolve("a", "local")).rejects.toBe("クラウドに繋がりません");
    expect(w.sync.current.rides.a).toBe("conflict");
    expect([w.local.get("a"), w.remote.get("a")]).toEqual(["t2", "t9"]);
  });
});

describe("走行を消す", () => {
  test("クラウドから消し、待っていた送信はやめる", async () => {
    const w = world();
    await w.sync.start();
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    w.save("a", "t2");
    await w.sync.remove("a");
    w.local.delete("a");
    w.sync.forget("a");
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.remote.has("a")).toBe(false);
    expect(w.sync.current.rides).toEqual({});
    expect(w.bar()).toBe("synced");
  });

  test("送っている最中なら、済むのを待ってから消す", async () => {
    const w = world();
    await w.sync.start();
    let open = () => {};
    w.state.gate = new Promise((resolve) => (open = resolve));
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    const removing = w.sync.remove("a");
    await settle();
    expect(w.log).not.toContain("remove a");
    w.state.gate = undefined;
    open();
    await removing;
    expect(w.log.slice(-2)).toEqual(["push a t1", "remove a"]);
    expect(w.remote.has("a")).toBe(false);
  });

  test("クラウドから消せなければ投げ、送らずに置いた保存を送り直す", async () => {
    const w = world();
    await w.sync.start();
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    w.state.online = false;
    w.save("a", "t2");
    await expect(w.sync.remove("a")).rejects.toBe("クラウドに繋がりません");
    w.state.online = true;
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.remote.get("a")).toBe("t2");
    expect(w.sync.current.rides.a).toBe("synced");
  });

  test("送れずにいる走行を消せなかったあとも、もう一度試す予約が残る", async () => {
    const w = world();
    await w.sync.start();
    w.state.pushable = false;
    w.save("a", "t1");
    await w.time.advance(PUSH_DELAY_MS);
    expect(w.sync.current.rides.a).toBe("unsent");
    // 消している間に突き合わせが走り、そのあとで消すのに失敗する。
    const open = w.plug("removeGate");
    const removing = w.sync.remove("a");
    await w.sync.check();
    w.state.removable = false;
    open();
    await expect(removing).rejects.toBe("クラウドが 500 を返しました");
    w.state.pushable = true;
    await w.time.advance(RETRY_DELAY_MS);
    expect(w.remote.get("a")).toBe("t1");
    expect(w.bar()).toBe("synced");
  });

  test("同期を設定していなければ、クラウドには触らない", async () => {
    const w = world();
    w.state.configured = false;
    w.state.online = false;
    await w.sync.start();
    await w.sync.remove("a");
    w.sync.forget("a");
    expect(w.log).toEqual([]);
  });
});
