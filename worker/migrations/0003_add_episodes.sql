-- 回（YouTube に上げる 1 本ぶんの範囲）の並び。走行あたり数件で、Worker は中身を見ないので、
-- 表に分けず JSON で持つ。
ALTER TABLE rides ADD COLUMN episodes TEXT NOT NULL DEFAULT '[]';
-- 経由点の ID。回の端を指すのに使う。この列を足す前の行と、ID を持たない古いアプリから
-- 送られた経由点では NULL。
ALTER TABLE waypoints ADD COLUMN id TEXT;
