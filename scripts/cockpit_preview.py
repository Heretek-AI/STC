#!/usr/bin/env python3
# Cockpit browser-dev preview (issue #17): serves cockpit/dist with a live
# /snapshot route backed by `studio snapshot --db` (real projection data,
# never mocks). Usage: scripts/cockpit_preview.py --db studio.db [--port 4173]
import argparse, functools, http.server, json, subprocess, threading, time

SNAPSHOT = {
    "at": 0.0,
    "doc": {
        "db_age_ms": 0,
        "source": "studio.db",
        "fleet": [],
        "stream": [],
        "receipts": [],
        "burn": [],
        "change_seq": 0,
        "runtime_mode": "rootless",
        "autonomy_mode": "full",
    },
}


def refresh(cli, db):
    while True:
        try:
            out = subprocess.run(
                [cli, "snapshot", "--db", db],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if out.returncode == 0 and out.stdout.strip():
                SNAPSHOT["doc"] = json.loads(out.stdout)
                SNAPSHOT["at"] = time.time()
        except Exception as e:
            SNAPSHOT["doc"] = {"error": str(e)}
        time.sleep(1)


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/snapshot":
            body = json.dumps(SNAPSHOT["doc"]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        return super().do_GET()

    def log_message(self, *a):
        pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", required=True)
    ap.add_argument("--port", type=int, default=4173)
    ap.add_argument("--dir", default="cockpit/dist")
    ap.add_argument("--cli", default="target/debug/studio-cli")
    args = ap.parse_args()
    threading.Thread(target=refresh, args=(args.cli, args.db), daemon=True).start()
    handler = functools.partial(Handler, directory=args.dir)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), handler)
    print(f"preview on 127.0.0.1:{args.port} db={args.db}", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
