#!/usr/bin/env bash
# Runs ON the Siam host: signs <file> in place with the SSL.com eSigner certificate, through the signing tool
# Siam App uses (sign-artifact.py --batch: saved credentials + TOTP, nothing to type). The tool writes the signed
# copy into its own run folder and a receipt; the newest verified receipt for these source bytes names it.
# The tool only takes .exe / .dll names, so other names (makensis' temporary uninstaller) go through an .exe copy.
#   bash siam-sign.sh <file>
set -euo pipefail
F=$1
TOOL=${SIGN_TOOL:-/root/siam-app/scripts/signing/sign-artifact.py}
BASE=${SIGN_BASE:-/root/.local/share/siam-signing}
SRC=$F
TMP=
case "$F" in
  *.exe|*.dll) ;;
  *)
    TMP=$(mktemp -d)
    SRC="$TMP/$(basename "$F").exe"
    cp "$F" "$SRC"
    ;;
esac
python3 "$TOOL" --batch "$SRC" >&2
SIGNED=$(SIGN_BASE="$BASE" python3 - "$SRC" <<'PY'
import hashlib, json, os, pathlib, sys
sha = hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest()
runs = sorted(pathlib.Path(os.environ["SIGN_BASE"]).glob("manual-*/receipt.json"), key=lambda p: p.stat().st_mtime, reverse=True)
for r in runs:
    j = json.loads(r.read_text())
    if j.get("sourceSha256") == sha and j.get("status") == "signed-linux-verified":
        print(j["signed"])
        break
else:
    sys.exit("no verified signature for " + sys.argv[1])
PY
)
cp "$SIGNED" "$F"
[ -n "$TMP" ] && rm -rf "$TMP"
echo "signed: $F" >&2
