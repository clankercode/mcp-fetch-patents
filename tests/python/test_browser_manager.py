"""Tests for browser lifecycle manager."""
from __future__ import annotations

from unittest.mock import MagicMock, patch
import pytest

from patent_mcp.search.profile_manager import ProfileManager
from patent_mcp.search.browser_manager import BrowserManager, BrowserNotAvailableError


@pytest.fixture
def pm(tmp_path):
    return ProfileManager(profiles_dir=tmp_path / "profiles")


class TestBrowserManagerInit:
    def test_not_running_initially(self, pm):
        bm = BrowserManager(pm, idle_timeout=5.0)
        assert not bm.is_running

    def test_close_when_not_running(self, pm):
        bm = BrowserManager(pm, idle_timeout=5.0)
        bm.close()  # should not raise


class TestBrowserManagerWithoutPlaywright:
    def test_get_page_fails_without_playwright(self, pm):
        bm = BrowserManager(pm, idle_timeout=5.0)
        with patch.dict("sys.modules", {"playwright": None, "playwright.sync_api": None}):
            # Force re-check of playwright availability
            import patent_mcp.search.browser_manager as bmod
            old = bmod._PLAYWRIGHT_MISSING
            bmod._PLAYWRIGHT_MISSING = True
            try:
                with pytest.raises(BrowserNotAvailableError, match="Playwright"):
                    bm.get_page()
            finally:
                bmod._PLAYWRIGHT_MISSING = old


class TestBrowserManagerMocked:
    """Test browser lifecycle with mocked Playwright."""

    def _make_mock_playwright(self):
        """Create mock Playwright objects."""
        mock_pw = MagicMock()
        mock_context = MagicMock()
        mock_page = MagicMock()
        mock_context.new_page.return_value = mock_page

        mock_pw_instance = MagicMock()
        mock_pw_instance.chromium.launch_persistent_context.return_value = mock_context
        mock_pw.return_value.start.return_value = mock_pw_instance

        return mock_pw, mock_pw_instance, mock_context, mock_page

    @patch("patent_mcp.search.browser_manager.BrowserManager._start_idle_timer")
    def test_get_page_starts_browser(self, mock_timer, pm):
        bm = BrowserManager(pm, idle_timeout=5.0)
        mock_pw, mock_pw_inst, mock_ctx, mock_page = self._make_mock_playwright()

        with patch("patent_mcp.search.browser_manager.sync_playwright", create=True) as mock_sp:
            # We need to patch the import inside _ensure_started
            with patch.dict("sys.modules", {}):
                # Simulate successful browser start by directly setting internal state
                bm._pw = mock_pw_inst
                bm._context = mock_ctx
                bm._pm.acquire_lock("default", "search")

                page = bm.get_page()
                assert page == mock_page
                assert bm.is_running

                bm.release_page(page)
                mock_page.close.assert_called_once()

    @patch("patent_mcp.search.browser_manager.BrowserManager._start_idle_timer")
    def test_close_releases_lock(self, mock_timer, pm):
        bm = BrowserManager(pm, idle_timeout=5.0)
        mock_ctx = MagicMock()
        mock_pw_inst = MagicMock()

        bm._context = mock_ctx
        bm._pw = mock_pw_inst
        bm._pm.acquire_lock("default", "search")

        bm.close()
        assert not bm.is_running
        locked, _ = pm.is_locked("default")
        assert not locked

    @patch("patent_mcp.search.browser_manager.BrowserManager._start_idle_timer")
    def test_multiple_pages(self, mock_timer, pm):
        """Multiple pages can be opened from the same context."""
        bm = BrowserManager(pm, idle_timeout=5.0)
        mock_ctx = MagicMock()
        pages = [MagicMock(), MagicMock()]
        mock_ctx.new_page.side_effect = pages

        bm._context = mock_ctx
        bm._pw = MagicMock()
        bm._pm.acquire_lock("default", "search")

        p1 = bm.get_page()
        p2 = bm.get_page()
        assert p1 == pages[0]
        assert p2 == pages[1]

        bm.release_page(p1)
        bm.release_page(p2)
        bm.close()
