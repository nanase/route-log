-- 走行のラベル。文字列の配列を JSON で持つ。ラベルで探すことは Worker ではしないので、表に分けない。
ALTER TABLE rides ADD COLUMN labels TEXT NOT NULL DEFAULT '[]';
