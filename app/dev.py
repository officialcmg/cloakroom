"""Run the Cloakroom app locally, end to end.

Starts the local test chains (Zebra regtest in Docker, anvil with the escrow and a mock USDC),
creates a funded maker and taker, and opens one app for each:

    maker  http://127.0.0.1:7801/
    taker  http://127.0.0.1:7802/

Press Ctrl-C to stop everything. Needs what harness/run.py needs: Docker, Foundry, and a built
node (cd node && cargo build).
"""
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROOT, "harness"))
import run as H  # noqa: E402  (the harness: chains, parties, funding)

H.EVID = os.path.join(H.WORK, "app-evidence")  # keep demo logs out of the committed evidence


def main():
    H.start_chains(nu7=False)
    maker, taker = H.parties("app")
    procs = []
    try:
        for party, port in ((maker, 7801), (taker, 7802)):
            procs.append(subprocess.Popen([H.BIN, "serve", "--dir", party.dir, "--port", str(port),
                                           "--ui", os.path.join(HERE, "ui"), "--ds", os.path.join(ROOT, "design-system")]))
        H.log("maker app: http://127.0.0.1:7801/   taker app: http://127.0.0.1:7802/   (Ctrl-C stops everything)")
        H.log("in the maker app press Start quoting, then start a swap in the taker app")
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        for p in procs:
            p.terminate()
        subprocess.run(["pkill", "-f", os.path.join(H.WORK, "app")], capture_output=True)
        H.stop_chains()


if __name__ == "__main__":
    main()
