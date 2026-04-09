"""Browser profile management with file-based locking.

Profiles are persistent Chromium user-data directories stored under
~/.local/share/patent-search/browser-profiles/<name>/. A lock file
prevents two processes from using the same profile simultaneously.
"""

from __future__ import annotations

import json
import logging
import os
import socket
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

log = logging.getLogger(__name__)


def _default_profiles_dir() -> Path:
    xdg = os.environ.get("XDG_DATA_HOME", "")
    base = Path(xdg) if xdg else Path.home() / ".local" / "share"
    return base / "patent-search" / "browser-profiles"


# ---------------------------------------------------------------------------
# Lock data
# ---------------------------------------------------------------------------


@dataclass
class ProfileLock:
    pid: int
    hostname: str
    started_at: str
    purpose: str  # "login" or "search"


class ProfileBusyError(Exception):
    """Raised when a profile is locked by another process."""

    def __init__(self, name: str, lock: ProfileLock) -> None:
        self.profile_name = name
        self.lock = lock
        super().__init__(
            f"Profile '{name}' is busy ({lock.purpose}, pid={lock.pid}, "
            f"host={lock.hostname}, since={lock.started_at})"
        )


# ---------------------------------------------------------------------------
# ProfileManager
# ---------------------------------------------------------------------------


class ProfileManager:
    """Manage Chromium browser profile directories and their locking."""

    def __init__(self, profiles_dir: Path | None = None) -> None:
        self._dir = profiles_dir or _default_profiles_dir()
        self._dir.mkdir(parents=True, exist_ok=True)

    @property
    def profiles_dir(self) -> Path:
        return self._dir

    # ------------------------------------------------------------------
    # Profile directory
    # ------------------------------------------------------------------

    def get_profile_dir(self, name: str = "default") -> Path:
        """Return (and create) the profile directory for *name*."""
        self._validate_name(name)
        d = self._dir / name
        d.mkdir(parents=True, exist_ok=True)
        return d

    @staticmethod
    def _validate_name(name: str) -> None:
        if not name:
            raise ValueError("Profile name cannot be empty")
        if any(c in name for c in ("/", "\\", "\0")) or ".." in name:
            raise ValueError(f"Invalid profile name: {name!r}")

    def list_profiles(self) -> list[str]:
        """Return names of all profile directories."""
        if not self._dir.exists():
            return []
        return sorted(
            p.name
            for p in self._dir.iterdir()
            if p.is_dir() and not p.name.startswith(".")
        )

    # ------------------------------------------------------------------
    # Locking
    # ------------------------------------------------------------------

    def _lock_path(self, name: str) -> Path:
        return self._dir / name / ".lock"

    def acquire_lock(self, name: str, purpose: str) -> None:
        """Acquire the profile lock atomically. Raises ProfileBusyError if already held.

        Uses O_CREAT|O_EXCL for atomic create-or-fail to avoid TOCTOU races
        between concurrent processes.
        """
        self.get_profile_dir(name)
        lp = self._lock_path(name)

        lock_data = {
            "pid": os.getpid(),
            "hostname": socket.gethostname(),
            "started_at": datetime.now(tz=timezone.utc).isoformat(),
            "purpose": purpose,
        }
        lock_bytes = json.dumps(lock_data).encode("utf-8")

        try:
            fd = os.open(str(lp), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
            try:
                os.write(fd, lock_bytes)
            finally:
                os.close(fd)
        except FileExistsError:
            # Lock file exists — check if it's stale
            locked, existing = self.is_locked(name)
            if locked and existing is not None:
                raise ProfileBusyError(name, existing)
            # Stale lock was cleared by is_locked; retry once
            try:
                fd = os.open(str(lp), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
                try:
                    os.write(fd, lock_bytes)
                finally:
                    os.close(fd)
            except FileExistsError:
                # Another process grabbed it between stale-clear and retry
                locked2, existing2 = self.is_locked(name)
                if existing2 is not None:
                    raise ProfileBusyError(name, existing2)
                raise ProfileBusyError(
                    name,
                    ProfileLock(
                        pid=0,
                        hostname="unknown",
                        started_at="",
                        purpose="unknown",
                    ),
                )

    def release_lock(self, name: str) -> None:
        """Release the profile lock (only if we own it)."""
        lp = self._lock_path(name)
        if not lp.exists():
            return
        try:
            data = json.loads(lp.read_text(encoding="utf-8"))
            if (
                data.get("pid") == os.getpid()
                and data.get("hostname") == socket.gethostname()
            ):
                lp.unlink(missing_ok=True)
            else:
                log.warning(
                    "Not releasing lock for profile '%s' — owned by pid=%s on %s",
                    name,
                    data.get("pid"),
                    data.get("hostname"),
                )
        except Exception:
            log.warning(
                "Failed to parse lock file for profile '%s'", name, exc_info=True
            )

    def is_locked(self, name: str) -> tuple[bool, ProfileLock | None]:
        """Check if a profile is locked. Clears stale locks (dead PID, same host)."""
        lp = self._lock_path(name)
        if not lp.exists():
            return False, None

        try:
            data = json.loads(lp.read_text(encoding="utf-8"))
            lock = ProfileLock(
                pid=data["pid"],
                hostname=data["hostname"],
                started_at=data["started_at"],
                purpose=data["purpose"],
            )
        except Exception:
            # Corrupt lock file — remove it
            lp.unlink(missing_ok=True)
            return False, None

        # Stale-lock recovery: clear if PID is gone on the same host
        if lock.hostname == socket.gethostname():
            if not _pid_alive(lock.pid):
                log.info(
                    "Clearing stale lock for profile '%s' (pid=%d no longer running)",
                    name,
                    lock.pid,
                )
                lp.unlink(missing_ok=True)
                return False, None

        return True, lock

    def force_release_lock(self, name: str) -> None:
        """Force-remove the lock file regardless of ownership."""
        lp = self._lock_path(name)
        lp.unlink(missing_ok=True)


def _pid_alive(pid: int) -> bool:
    """Check if a process with the given PID is running."""
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        # Process exists but we can't signal it
        return True
