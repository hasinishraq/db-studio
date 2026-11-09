"""Demo data for screenshots: a SQLite file of tasks. Generated, deterministic.

Usage: python sqlite-notes.py <path/to/notes.db>
"""
import sqlite3
import sys

path = sys.argv[1]
db = sqlite3.connect(path)
db.executescript("""
DROP TABLE IF EXISTS task_tags; DROP TABLE IF EXISTS attachments; DROP TABLE IF EXISTS tasks; DROP TABLE IF EXISTS tags;
CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, colour TEXT NOT NULL);
CREATE TABLE tasks (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('todo', 'doing', 'done')),
  priority INTEGER NOT NULL,
  due_on TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE task_tags (
  task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id),
  PRIMARY KEY (task_id, tag_id)
);
CREATE TABLE attachments (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL REFERENCES tasks(id),
  file_name TEXT NOT NULL,
  content BLOB NOT NULL
);
""")
tags = ["work", "home", "urgent", "ideas", "reading", "errands", "health", "finance"]
db.executemany("INSERT INTO tags (id, name, colour) VALUES (?, ?, ?)",
               [(i + 1, t, ["blue", "green", "red", "violet", "orange", "teal", "pink", "grey"][i]) for i, t in enumerate(tags)])
verbs = ["Write", "Review", "Plan", "Call", "Fix", "Read", "Pay", "Book", "Clean", "Draft"]
things = ["quarterly report", "dentist", "garden shed", "budget sheet", "team retro", "train tickets",
          "kitchen tap", "tax papers", "reading list", "birthday gift"]
tasks = []
for g in range(1, 2001):
    status = ["todo", "doing", "done", "done"][g % 4]
    due = None if g % 5 == 0 else f"2026-{(g % 12) + 1:02d}-{(g % 28) + 1:02d}"
    tasks.append((g, f"{verbs[g % 10]} {things[(g * 3) % 10]} #{g}", status, g % 4 + 1, due,
                  f"2026-01-01 {g % 24:02d}:{g % 60:02d}:00"))
db.executemany("INSERT INTO tasks VALUES (?, ?, ?, ?, ?, ?)", tasks)
db.executemany("INSERT INTO task_tags VALUES (?, ?)", [(g, (g * 7) % 8 + 1) for g in range(1, 2001)])
db.executemany("INSERT INTO attachments (task_id, file_name, content) VALUES (?, ?, ?)",
               [(g * 9 % 2000 + 1, f"scan-{g}.pdf", bytes((g + i) % 256 for i in range(512))) for g in range(1, 201)])
db.commit()
db.close()
print("wrote", path)
