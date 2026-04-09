"""Browser lifecycle manager — start once, reuse, auto-close after idle timeout.

Uses Playwright's sync API with persistent Chromium contexts so that cookies,
local-storage, and login state survive across search calls. A background thread
monitors idle time and closes the browser after the configured timeout (default
30 minutes).
"""
from __future__ import annotations

import atexit
import logging
import threading
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from patent_mcp.search.profile_manager import ProfileManager

log = logging.getLogger(__name__)

# Sentinel for "Playwright is not installed"
_PLAYWRIGHT_MISSING = False


class BrowserNotAvailableError(Exception):
    """Raised when Playwright is not installed or browser can't start."""


class BrowserManager:
    """Manage a single long-lived Playwright browser with idle-timeout shutdown.

    Thread-safe: multiple search calls can request pages concurrently.  The
    browser starts lazily on the first ``get_page()`` call and shuts down
    automatically when it hasn't been used for ``idle_timeout`` seconds.
    """

    def __init__(
        self,
        profile_manager: "ProfileManager",
        profile_name: str = "default",
        headless: bool = True,
        idle_timeout: float = 1800.0,  # 30 minutes
        timeout: float = 60_000,  # Playwright navigation timeout, ms
    ) -> None:
        self._pm = profile_manager
        self._profile_name = profile_name
        self._headless = headless
        self._idle_timeout = idle_timeout
        self._nav_timeout = timeout

        # Guards all mutable state below
        self._lock = threading.Lock()

        # Playwright objects (None when browser not running)
        self._pw: Any = None
        self._context: Any = None
        self._last_used: float = 0.0

        # Idle-timer thread
        self._idle_thread: threading.Thread | None = None
        self._stop_event = threading.Event()

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    @property
    def is_running(self) -> bool:
        with self._lock:
            return self._context is not None

    def get_page(self) -> Any:
        """Return a new Page from the persistent context. Starts browser if needed.

        The caller MUST call ``release_page(page)`` when done.
        """
        with self._lock:
            self._ensure_started()
            self._last_used = time.monotonic()
            return self._context.new_page()

    def release_page(self, page: Any) -> None:
        """Close a page and update the idle timestamp."""
        try:
            page.close()
        except Exception:
            pass
        with self._lock:
            self._last_used = time.monotonic()

    def close(self) -> None:
        """Shut down the browser and release the profile lock."""
        with self._lock:
            self._close_internal()

    # ------------------------------------------------------------------
    # Internals — must be called with self._lock held
    # ------------------------------------------------------------------

    def _ensure_started(self) -> None:
        """Start Playwright + browser if not already running."""
        if self._context is not None:
            return

        global _PLAYWRIGHT_MISSING
        if _PLAYWRIGHT_MISSING:
            raise BrowserNotAvailableError(
                "Playwright is not installed. Run: pip install patent-mcp-server[browser] && playwright install chromium"
            )

        try:
            from playwright.sync_api import sync_playwright
        except ImportError:
            _PLAYWRIGHT_MISSING = True
            raise BrowserNotAvailableError(
                "Playwright is not installed. Run: pip install patent-mcp-server[browser] && playwright install chromium"
            )

        profile_dir = self._pm.get_profile_dir(self._profile_name)

        # Acquire the profile lock (raises ProfileBusyError if held)
        self._pm.acquire_lock(self._profile_name, "search")

        try:
            self._pw = sync_playwright().start()
            self._context = self._pw.chromium.launch_persistent_context(
                user_data_dir=str(profile_dir),
                headless=self._headless,
                # Reasonable defaults
                viewport={"width": 1280, "height": 900},
                user_agent=(
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
                    "(KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36"
                ),
                java_script_enabled=True,
                accept_downloads=False,
            )
            self._context.set_default_navigation_timeout(self._nav_timeout)
            self._last_used = time.monotonic()
            self._start_idle_timer()
            atexit.register(self.close)
            log.info(
                "Browser started (profile=%s, headless=%s, idle_timeout=%ds)",
                self._profile_name, self._headless, int(self._idle_timeout),
            )
        except Exception:
            # Clean up on failure
            if self._pw:
                try:
                    self._pw.stop()
                except Exception:
                    pass
                self._pw = None
            self._pm.release_lock(self._profile_name)
            raise

    def _close_internal(self) -> None:
        """Shut everything down. Must be called with self._lock held."""
        self._stop_event.set()

        if self._context:
            try:
                self._context.close()
            except Exception as e:
                log.debug("Error closing browser context: %s", e)
            self._context = None

        if self._pw:
            try:
                self._pw.stop()
            except Exception as e:
                log.debug("Error stopping playwright: %s", e)
            self._pw = None

        try:
            self._pm.release_lock(self._profile_name)
        except Exception:
            pass

        log.info("Browser closed (profile=%s)", self._profile_name)

    # ------------------------------------------------------------------
    # Idle timer
    # ------------------------------------------------------------------

    def _start_idle_timer(self) -> None:
        """Spawn (or replace) the idle-monitor daemon thread."""
        self._stop_event.clear()
        if self._idle_thread and self._idle_thread.is_alive():
            return
        self._idle_thread = threading.Thread(
            target=self._idle_loop, daemon=True, name="browser-idle-timer",
        )
        self._idle_thread.start()

    def _idle_loop(self) -> None:
        """Periodically check if the browser has been idle too long."""
        check_interval = min(60.0, self._idle_timeout / 4)
        while not self._stop_event.is_set():
            self._stop_event.wait(timeout=check_interval)
            if self._stop_event.is_set():
                return
            with self._lock:
                if self._context is None:
                    return
                idle_secs = time.monotonic() - self._last_used
                if idle_secs >= self._idle_timeout:
                    log.info(
                        "Browser idle for %ds — shutting down (profile=%s)",
                        int(idle_secs), self._profile_name,
                    )
                    self._close_internal()
                    return
