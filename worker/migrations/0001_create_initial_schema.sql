-- 走行。経路の線は R2 に置き、ここにはそのキーだけを持つ。
CREATE TABLE rides (
  id TEXT PRIMARY KEY,            -- UUIDv7
  title TEXT NOT NULL,
  ride_date TEXT NOT NULL,        -- YYYY-MM-DD (JST)
  clock_offset_ms INTEGER NOT NULL DEFAULT 0,
  is_public INTEGER NOT NULL DEFAULT 0,
  route_key TEXT,                 -- R2 のオブジェクトキー
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE waypoints (
  ride_id TEXT NOT NULL REFERENCES rides(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  lon REAL NOT NULL,
  lat REAL NOT NULL,
  segment_mode TEXT NOT NULL,     -- 次の経由点までの結び方
  PRIMARY KEY (ride_id, seq)
);

-- イベント。絞り込みに使う値を列に出し、全体は json に持つ。
-- 項目が増えても、列を足さずに往復できる。
CREATE TABLE events (
  id TEXT PRIMARY KEY,
  ride_id TEXT NOT NULL REFERENCES rides(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,           -- 保存したときの並び
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  route_m REAL NOT NULL,
  time_ms INTEGER,                -- Unix ms
  status TEXT NOT NULL,
  json TEXT NOT NULL
);

CREATE INDEX idx_events_ride ON events(ride_id, route_m);
