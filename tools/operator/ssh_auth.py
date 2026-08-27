"""Shared SSH authentication for the HomBot operator tools.

The device runs Dropbear 2013.56. Its public-key path needs an RSA key and the
legacy ``ssh-rsa`` signature algorithm. A dedicated key is preferred; the
existing password/secret-file loaders remain available as a fallback.
"""

import os
from pathlib import Path

import paramiko


DEFAULT_KEY = Path.home() / ".ssh" / "hombot_codex_rsa"


def connect_auth(password_loader):
    """Return Paramiko connect kwargs without exposing credential material."""

    configured = os.environ.get("HOMBOT_SSH_KEY")
    key_path = Path(configured).expanduser() if configured else DEFAULT_KEY
    if key_path.is_file():
        key = paramiko.RSAKey.from_private_key_file(str(key_path))
        return {
            "pkey": key,
            "look_for_keys": False,
            "allow_agent": False,
            "disabled_algorithms": {
                "pubkeys": ["rsa-sha2-512", "rsa-sha2-256"],
            },
        }

    return {
        "password": password_loader(),
        "look_for_keys": False,
        "allow_agent": False,
    }

