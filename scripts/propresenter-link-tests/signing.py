"""TEST ONLY. A throwaway Ed25519 key pair and a signer, so the tests can sign update bundles without the project's private key
(which is never in this repository and never on a test machine). The helper only verifies; this file signs, with the plain reference
algorithm of RFC 8032 section 5.1.6 on the helper's own curve arithmetic (and it is checked against the RFC's vectors)."""
import base64
import hashlib
import json

import sys

sys.dont_write_bytecode = True      # never leave a __pycache__ inside the helper folder: it is a bundle of plain files
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "crates", "stagepilot-propresenter", "assets", "propresenter-link"))
import propresenter_link as lk  # noqa: E402

# The seed of the test key pair. Anyone can read it: it signs nothing but test bundles, and the helper accepts the matching public
# key only while a test sets it on the module (lk.UPDATE_PUBLIC_KEY = ...), which no request and no config file can do.
TEST_SEED = hashlib.sha256(b"propresenter-link TEST ONLY signing key").digest()
OTHER_SEED = hashlib.sha256(b"propresenter-link TEST ONLY second key").digest()


def _encode(point):
    zi = pow(point[2], lk._ED_P - 2, lk._ED_P)
    x, y = point[0] * zi % lk._ED_P, point[1] * zi % lk._ED_P
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def public_key(seed):
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little") & ((1 << 254) - 8) | (1 << 254)
    return _encode(lk._ed_mul(a, lk._ED_BASE))


def sign(seed, message):
    """The 64-byte Ed25519 signature of `message` by the key made from `seed`."""
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little") & ((1 << 254) - 8) | (1 << 254)
    pub = _encode(lk._ed_mul(a, lk._ED_BASE))
    r = int.from_bytes(hashlib.sha512(h[32:] + message).digest(), "little") % lk._ED_L
    big_r = _encode(lk._ed_mul(r, lk._ED_BASE))
    k = int.from_bytes(hashlib.sha512(big_r + pub + message).digest(), "little") % lk._ED_L
    return big_r + ((r + k * a) % lk._ED_L).to_bytes(32, "little")


def public_b64(seed=TEST_SEED):
    return base64.b64encode(public_key(seed)).decode()


def sums_text(files, extra=None):
    """A SHA256SUMS text for {name: bytes}; `extra` adds names (install.sh, ...) that are listed but not sent."""
    table = {n: hashlib.sha256(b).hexdigest() for n, b in files.items()}
    table.update({n: hashlib.sha256(b).hexdigest() for n, b in (extra or {}).items()})
    return "".join("%s  %s\n" % (table[n], n) for n in sorted(table))


def bundle(files, extra=None, seed=TEST_SEED, sums=None):
    """The /update request body (as a dict) for {name: bytes}, signed by `seed`."""
    text = sums if sums is not None else sums_text(files, extra)
    return {"files": {n: base64.b64encode(b).decode() for n, b in files.items()}, "sums": text,
            "sig": base64.b64encode(sign(seed, text.encode("utf-8"))).decode()}


def body(request):
    return json.dumps(request).encode("utf-8")
