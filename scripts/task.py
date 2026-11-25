#!/usr/bin/env python3
"""M1 task tracker: file-per-task + JSONL events, stdlib only.

Why not sqlite: git-diffable, no new deps, agent writes via atomic
rename + append-only log. SQL-when-needed: load JSON in-memory.
"""
import argparse
import datetime
import json
import os
import re
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TASKS_DIR = os.path.join(ROOT, "tasks")
EVENTS = os.path.join(TASKS_DIR, "events.jsonl")
MEMORY = os.path.join(ROOT, "MEMORY.md")

VALID = ("todo", "doing", "done", "blocked")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9\-]{1,60}$")


def utcnow():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")


def iso_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def task_path(tid):
    return os.path.join(TASKS_DIR, tid + ".json")


def load_task(tid):
    p = task_path(tid)
    if not os.path.exists(p):
        return None
    with open(p, encoding="utf-8") as f:
        return json.load(f)


def atomic_write(path, obj):
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as f:
            json.dump(obj, f, ensure_ascii=False, indent=2)
            f.write("\n")
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def append_event(agent, task, status, detail="", files=""):
    os.makedirs(TASKS_DIR, exist_ok=True)
    ev = {
        "ts": utcnow(),
        "agent": agent,
        "task": task,
        "status": status,
        "detail": detail,
        "files": files,
    }
    line = json.dumps(ev, ensure_ascii=False)
    with open(EVENTS, "a", encoding="utf-8", newline="\n") as f:
        f.write(line + "\n")
    # transition dual-write: keep MEMORY.md human-readable
    try:
        with open(MEMORY, "a", encoding="utf-8", newline="\n") as f:
            f.write("- [%s] agent=%s task=%s status=%s detail=%s files=%s\n"
                    % (ev["ts"], agent, task, status, detail, files))
    except OSError:
        pass
    return ev


def cmd_list(args):
    rows = []
    for fn in sorted(os.listdir(TASKS_DIR)):
        if not fn.endswith(".json"):
            continue
        with open(os.path.join(TASKS_DIR, fn), encoding="utf-8") as f:
            try:
                t = json.load(f)
            except json.JSONDecodeError:
                continue
        if args.status and t.get("status") != args.status:
            continue
        rows.append(t)
    rows.sort(key=lambda t: (t.get("status", ""), t.get("id", "")))
    for t in rows:
        print("%-24s %-8s %-10s %s" % (t.get("id"), t.get("status"), t.get("owner", "-"), t.get("title", "")))
    if args.json:
        print(json.dumps(rows, ensure_ascii=False, indent=2))


def cmd_show(args):
    t = load_task(args.id)
    if not t:
        print("not found: %s" % args.id, file=sys.stderr)
        return 1
    print(json.dumps(t, ensure_ascii=False, indent=2))


def cmd_new(args):
    if not ID_RE.match(args.id):
        print("bad id (lowercase, digits, dashes): %s" % args.id, file=sys.stderr)
        return 2
    p = task_path(args.id)
    if os.path.exists(p):
        print("exists: %s" % args.id, file=sys.stderr)
        return 1
    now = iso_now()
    t = {
        "id": args.id,
        "title": args.title,
        "status": "todo",
        "owner": args.owner or "",
        "priority": args.priority or "p2",
        "deps": args.dep or [],
        "branch": args.branch or "",
        "created": now,
        "updated": now,
        "detail": "",
        "files": [],
    }
    atomic_write(p, t)
    append_event(args.owner or "main", args.id, "todo", args.title, "")
    print("created %s" % args.id)


def cmd_claim(args):
    t = load_task(args.id)
    if not t:
        print("not found: %s" % args.id, file=sys.stderr)
        return 1
    if t.get("status") == "doing" and t.get("owner") and t.get("owner") != args.owner:
        print("busy: owned by %s (use --force to steal)" % t.get("owner"), file=sys.stderr)
        return 1
    if t.get("status") == "done" and not args.force:
        print("already done (use --force to reopen)", file=sys.stderr)
        return 1
    # unmet deps guard (soft: warn, --force to override)
    unmet = [d for d in (t.get("deps") or []) if (load_task(d) or {}).get("status") != "done"]
    if unmet and not args.force:
        print("blocked by unfinished deps: %s (use --force)" % ",".join(unmet), file=sys.stderr)
        return 1
    t["status"] = "doing"
    t["owner"] = args.owner
    t["updated"] = iso_now()
    atomic_write(task_path(args.id), t)
    append_event(args.owner, args.id, "start", "claim", "")
    print("claimed %s by %s" % (args.id, args.owner))


def cmd_finish(args, status):
    t = load_task(args.id)
    if not t:
        print("not found: %s" % args.id, file=sys.stderr)
        return 1
    t["status"] = status
    t["updated"] = iso_now()
    if args.detail:
        t["detail"] = args.detail
    if args.files:
        t["files"] = [s.strip() for s in args.files.split(",") if s.strip()]
    atomic_write(task_path(args.id), t)
    append_event(args.owner or t.get("owner") or "main", args.id, status,
                 args.detail or "", args.files or "")
    print("%s %s" % (status, args.id))


def cmd_log(args):
    if not os.path.exists(EVENTS):
        return
    n = 0
    with open(EVENTS, encoding="utf-8", errors="replace") as f:
        lines = f.readlines()
    if args.task:
        lines = [l for l in lines if '"task": "%s"' % args.task in l]
    lines = lines[-args.limit:]
    for l in lines:
        try:
            e = json.loads(l)
            print("[%s] %s %s %s %s" % (e.get("ts"), e.get("agent"), e.get("task"), e.get("status"), e.get("detail", "")[:100]))
            n += 1
        except json.JSONDecodeError:
            continue


MEM_LINE = re.compile(r"^- \[(.*?)\] agent=(.*?) task=(.*?) status=(.*?) detail=(.*?) files=(.*)$")


def cmd_migrate(args):
    """MEMORY.md -> events.jsonl, idempotent via line dedup."""
    if not os.path.exists(MEMORY):
        print("no MEMORY.md", file=sys.stderr)
        return 1
    seen = set()
    if os.path.exists(EVENTS):
        with open(EVENTS, encoding="utf-8", errors="replace") as f:
            for l in f:
                seen.add(l.strip())
    added = 0
    with open(MEMORY, encoding="utf-8", errors="replace") as f:
        mem_lines = f.readlines()
    with open(EVENTS, "a", encoding="utf-8", newline="\n") as out:
        for line in mem_lines:
            m = MEM_LINE.match(line.strip())
            if not m:
                continue
            ev = {"ts": m.group(1), "agent": m.group(2), "task": m.group(3),
                  "status": m.group(4), "detail": m.group(5), "files": m.group(6)}
            s = json.dumps(ev, ensure_ascii=False)
            if s in seen:
                continue
            out.write(s + "\n")
            seen.add(s)
            added += 1
    print("migrated %d events (total %d)" % (added, len(seen)))


def main():
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass
    ap = argparse.ArgumentParser(prog="task.py")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("list"); p.add_argument("--status", default=""); p.add_argument("--json", action="store_true")
    p = sub.add_parser("show"); p.add_argument("id")
    p = sub.add_parser("new"); p.add_argument("--id", required=True); p.add_argument("--title", required=True)
    p.add_argument("--owner", default=""); p.add_argument("--priority", default="p2")
    p.add_argument("--dep", action="append", default=[]); p.add_argument("--branch", default="")
    p = sub.add_parser("claim"); p.add_argument("id"); p.add_argument("--owner", required=True); p.add_argument("--force", action="store_true")
    for name in ("done", "block"):
        p = sub.add_parser(name); p.add_argument("id"); p.add_argument("--detail", default=""); p.add_argument("--files", default=""); p.add_argument("--owner", default="")
    p = sub.add_parser("log"); p.add_argument("--task", default=""); p.add_argument("--limit", type=int, default=20)
    p = sub.add_parser("migrate")
    args = ap.parse_args()
    if args.cmd == "list":
        return cmd_list(args)
    if args.cmd == "show":
        return cmd_show(args)
    if args.cmd == "new":
        return cmd_new(args)
    if args.cmd == "claim":
        return cmd_claim(args)
    if args.cmd == "done":
        return cmd_finish(args, "done")
    if args.cmd == "block":
        return cmd_finish(args, "blocked")
    if args.cmd == "log":
        return cmd_log(args)
    if args.cmd == "migrate":
        return cmd_migrate(args)


if __name__ == "__main__":
    sys.exit(main() or 0)
