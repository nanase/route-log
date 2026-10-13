/** 再生の検証（マイルストーン 1）。`<video>` 1 つに対して、読み込み・再生・シークを測る。 */

export interface SeekResult {
  /** 動画の長さに対する位置（0〜1） */
  at: number;
  playing: boolean;
  /** `seeked` までの時間。届かなければ null */
  seekedMs: number | null;
  /** シーク後の最初のフレームが出るまでの時間。届かなければ null */
  frameMs: number | null;
}

export interface PlayResult {
  rate: number;
  wallSeconds: number;
  /** 動画の時刻が進んだ量。`wallSeconds * rate` に近いほどよい */
  advancedSeconds: number;
  frames: number;
  dropped: number;
}

export interface CheckReport {
  source: string;
  userAgent: string;
  error: string | null;
  metadataMs: number | null;
  canPlayMs: number | null;
  duration: number | null;
  width: number;
  height: number;
  plays: PlayResult[];
  seeks: SeekResult[];
  /** 取り出した 1 フレームの平均の明るさ（0〜255）。読めなければ理由 */
  frameLuma: number | string;
  /** 逆再生（負の再生速度）を試した結果 */
  reverse: string;
}

const TIMEOUT_MS = 20_000;

function once(target: EventTarget, name: string, timeoutMs = TIMEOUT_MS): Promise<boolean> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      target.removeEventListener(name, done);
      resolve(false);
    }, timeoutMs);
    function done() {
      clearTimeout(timer);
      resolve(true);
    }
    target.addEventListener(name, done, { once: true });
  });
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function nextFrame(video: HTMLVideoElement, timeoutMs = TIMEOUT_MS): Promise<boolean> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), timeoutMs);
    video.requestVideoFrameCallback(() => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

async function measurePlay(video: HTMLVideoElement, rate: number, wallSeconds: number): Promise<PlayResult> {
  video.playbackRate = rate;
  await video.play();
  // 再生し始めの立ち上がりを計測に含めない。
  await sleep(1000);
  const before = video.getVideoPlaybackQuality();
  const startTime = video.currentTime;
  const startWall = performance.now();
  await sleep(wallSeconds * 1000);
  const after = video.getVideoPlaybackQuality();
  const result = {
    rate,
    wallSeconds: (performance.now() - startWall) / 1000,
    advancedSeconds: video.currentTime - startTime,
    frames: after.totalVideoFrames - before.totalVideoFrames,
    dropped: after.droppedVideoFrames - before.droppedVideoFrames,
  };
  video.pause();
  video.playbackRate = 1;
  return result;
}

async function measureSeek(video: HTMLVideoElement, at: number, playing: boolean): Promise<SeekResult> {
  if (playing) await video.play();
  else video.pause();
  const start = performance.now();
  const seeked = once(video, "seeked").then((ok) => (ok ? performance.now() - start : null));
  const frame = nextFrame(video).then((ok) => (ok ? performance.now() - start : null));
  video.currentTime = video.duration * at;
  const [seekedMs, frameMs] = await Promise.all([seeked, frame]);
  video.pause();
  return { at, playing, seekedMs, frameMs };
}

async function measureReverse(video: HTMLVideoElement): Promise<string> {
  try {
    video.playbackRate = -1;
    await video.play();
    const start = video.currentTime;
    await sleep(2000);
    const moved = video.currentTime - start;
    return `${moved.toFixed(2)} 秒動いた（負なら逆再生できている）`;
  } catch (e) {
    return String(e);
  } finally {
    video.pause();
    video.playbackRate = 1;
  }
}

function frameLuma(video: HTMLVideoElement): number | string {
  try {
    const canvas = document.createElement("canvas");
    canvas.width = 160;
    canvas.height = 90;
    const context = canvas.getContext("2d");
    if (context === null) return "2d コンテキストを取れない";
    context.drawImage(video, 0, 0, canvas.width, canvas.height);
    const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
    let sum = 0;
    for (let i = 0; i < data.length; i += 4) sum += (data[i] + data[i + 1] + data[i + 2]) / 3;
    return sum / (data.length / 4);
  } catch (e) {
    return String(e);
  }
}

export async function runCheck(
  video: HTMLVideoElement,
  source: string,
  log: (line: string) => void,
): Promise<CheckReport> {
  const report: CheckReport = {
    source,
    userAgent: navigator.userAgent,
    error: null,
    metadataMs: null,
    canPlayMs: null,
    duration: null,
    width: 0,
    height: 0,
    plays: [],
    seeks: [],
    frameLuma: "未計測",
    reverse: "未計測",
  };
  const failed = new Promise<string>((resolve) => {
    video.addEventListener(
      "error",
      () => resolve(`MediaError ${video.error?.code}: ${video.error?.message ?? ""}`),
      { once: true },
    );
  });

  const steps = (async () => {
    const start = performance.now();
    const metadata = once(video, "loadedmetadata").then((ok) => (ok ? performance.now() - start : null));
    const canPlay = once(video, "canplay").then((ok) => (ok ? performance.now() - start : null));
    video.src = source;
    report.metadataMs = await metadata;
    report.canPlayMs = await canPlay;
    if (report.canPlayMs === null) {
      report.error = "canplay に届かない";
      return;
    }
    report.duration = video.duration;
    report.width = video.videoWidth;
    report.height = video.videoHeight;
    log(`読み込み ${report.canPlayMs.toFixed(0)} ms、${video.videoWidth}x${video.videoHeight}、${video.duration.toFixed(1)} 秒`);

    for (const rate of [1, 2, 4]) {
      const play = await measurePlay(video, rate, 5);
      report.plays.push(play);
      log(`再生 x${rate}: ${play.advancedSeconds.toFixed(2)} 秒進む / ${play.wallSeconds.toFixed(2)} 秒、落ち ${play.dropped}/${play.frames}`);
    }
    report.frameLuma = frameLuma(video);
    report.reverse = await measureReverse(video);
    log(`逆再生: ${report.reverse}`);

    // 再生中の回は別の位置へ飛ぶ。同じ位置では、停止中の回に読んだぶんが残っていて速く出る。
    const positions: [boolean, number[]][] = [
      [false, [0.5, 0.9, 0.1, 0.99, 0.25, 0.75]],
      [true, [0.4, 0.8, 0.2, 0.95, 0.3, 0.65]],
    ];
    for (const [playing, ats] of positions) {
      for (const at of ats) {
        const seek = await measureSeek(video, at, playing);
        report.seeks.push(seek);
        log(`シーク ${(at * 100).toFixed(0)}%${playing ? "（再生中）" : ""}: seeked ${seek.seekedMs?.toFixed(0) ?? "×"} ms、フレーム ${seek.frameMs?.toFixed(0) ?? "×"} ms`);
      }
    }
  })();

  const error = await Promise.race([steps.then(() => null), failed]);
  if (error !== null) report.error = error;
  return report;
}
