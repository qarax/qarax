"""Webhook receiver for demos/hooks/run.sh.

Prints each lifecycle hook delivery as it arrives, verifies its
X-Qarax-Signature (HMAC-SHA256 of the raw body, keyed with the hook's
secret), and appends "<vm> <previous>→<new> <verified|bad-signature>" to
the file named by $DELIVERIES so the demo can wait for each transition.

Configured through the environment: HOOK_SECRET, WEBHOOK_PORT, DELIVERIES.
"""

import hashlib
import hmac
import json
import os
from datetime import datetime
from http.server import BaseHTTPRequestHandler, HTTPServer

SECRET = os.environ["HOOK_SECRET"].encode()
DELIVERIES = os.environ["DELIVERIES"]
PORT = int(os.environ["WEBHOOK_PORT"])

BOLD, DIM, YELLOW, GREEN, RED, RESET = (
    "\033[1m",
    "\033[2m",
    "\033[1;33m",
    "\033[32m",
    "\033[31m",
    "\033[0m",
)


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        expected = "sha256=" + hmac.new(SECRET, raw, hashlib.sha256).hexdigest()
        verified = hmac.compare_digest(
            self.headers.get("X-Qarax-Signature", ""), expected
        )
        event = json.loads(raw)
        transition = f"{event['previous_status']}→{event['new_status']}"

        ts = datetime.now().strftime("%H:%M:%S.%f")[:-3]
        signature = (
            f"{GREEN}signature verified{RESET}"
            if verified
            else f"{RED}BAD SIGNATURE{RESET}"
        )
        tags = ",".join(event["tags"]) or "-"
        print(
            f"  {YELLOW}⚡ WEBHOOK{RESET} [{ts}] {BOLD}{event['vm_name']}{RESET}: "
            f"{BOLD}{transition}{RESET}  {DIM}tags={tags}{RESET}  {signature}",
            flush=True,
        )
        with open(DELIVERIES, "a", encoding="utf-8") as log:
            status = "verified" if verified else "bad-signature"
            log.write(f"{event['vm_name']} {transition} {status}\n")

        self.send_response(200)
        self.end_headers()

    def log_message(self, format, *args):  # noqa: A002 - matches the base signature
        """Silence the default per-request access log."""


HTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
