"""Tests for browser profile management and locking."""
from __future__ import annotations

import json
import os

import pytest
from patent_mcp.search.profile_manager import ProfileManager, ProfileBusyError, ProfileLock


@pytest.fixture
def pm(tmp_path):
    return ProfileManager(profiles_dir=tmp_path / "profiles")


class TestProfileDirectories:
    def test_creates_profiles_dir(self, pm):
        assert pm.profiles_dir.exists()

    def test_get_profile_dir_creates(self, pm):
        d = pm.get_profile_dir("test-profile")
        assert d.exists()
        assert d.name == "test-profile"

    def test_get_profile_dir_default(self, pm):
        d = pm.get_profile_dir()
        assert d.name == "default"

    def test_list_profiles_empty(self, pm):
        assert pm.list_profiles() == []

    def test_list_profiles_after_create(self, pm):
        pm.get_profile_dir("alpha")
        pm.get_profile_dir("beta")
        assert pm.list_profiles() == ["alpha", "beta"]

    def test_list_profiles_excludes_dotfiles(self, pm):
        pm.get_profile_dir("visible")
        (pm.profiles_dir / ".hidden").mkdir()
        assert pm.list_profiles() == ["visible"]


class TestLocking:
    def test_acquire_and_check(self, pm):
        pm.acquire_lock("default", "search")
        locked, lock = pm.is_locked("default")
        assert locked
        assert lock is not None
        assert lock.pid == os.getpid()
        assert lock.purpose == "search"

    def test_release_own_lock(self, pm):
        pm.acquire_lock("default", "search")
        pm.release_lock("default")
        locked, _ = pm.is_locked("default")
        assert not locked

    def test_double_acquire_raises(self, pm):
        pm.acquire_lock("default", "search")
        with pytest.raises(ProfileBusyError) as exc_info:
            pm.acquire_lock("default", "login")
        assert "default" in str(exc_info.value)

    def test_not_locked_by_default(self, pm):
        pm.get_profile_dir("test")
        locked, lock = pm.is_locked("test")
        assert not locked
        assert lock is None

    def test_lock_file_contents(self, pm):
        pm.acquire_lock("myprofile", "login")
        lock_path = pm.profiles_dir / "myprofile" / ".lock"
        assert lock_path.exists()
        data = json.loads(lock_path.read_text())
        assert data["pid"] == os.getpid()
        assert data["purpose"] == "login"
        assert "hostname" in data
        assert "started_at" in data

    def test_stale_lock_cleared(self, pm):
        """A lock from a dead PID on the same host should be auto-cleared."""
        pm.get_profile_dir("stale")
        lock_path = pm.profiles_dir / "stale" / ".lock"
        import socket
        lock_path.write_text(json.dumps({
            "pid": 99999999,  # almost certainly dead
            "hostname": socket.gethostname(),
            "started_at": "2020-01-01T00:00:00Z",
            "purpose": "search",
        }))
        locked, _ = pm.is_locked("stale")
        assert not locked  # stale lock should be cleared

    def test_remote_lock_not_cleared(self, pm):
        """A lock from a different host should not be cleared."""
        pm.get_profile_dir("remote")
        lock_path = pm.profiles_dir / "remote" / ".lock"
        lock_path.write_text(json.dumps({
            "pid": 1,
            "hostname": "some-other-host-that-is-not-this-one",
            "started_at": "2020-01-01T00:00:00Z",
            "purpose": "search",
        }))
        locked, lock = pm.is_locked("remote")
        assert locked
        assert lock.hostname == "some-other-host-that-is-not-this-one"

    def test_corrupt_lock_file_cleared(self, pm):
        pm.get_profile_dir("corrupt")
        lock_path = pm.profiles_dir / "corrupt" / ".lock"
        lock_path.write_text("not json at all")
        locked, _ = pm.is_locked("corrupt")
        assert not locked
        assert not lock_path.exists()

    def test_force_release(self, pm):
        pm.acquire_lock("default", "search")
        pm.force_release_lock("default")
        locked, _ = pm.is_locked("default")
        assert not locked

    def test_release_nonexistent_lock(self, pm):
        # Should not raise
        pm.release_lock("nonexistent")
