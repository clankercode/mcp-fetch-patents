"""HTTP/API patent sources: PPUBS, EPO OPS, BigQuery, Espacenet, WIPO, etc."""

from __future__ import annotations

import logging
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import TYPE_CHECKING, Any

import httpx
from tenacity import (
    retry,
    retry_if_exception,
    stop_after_attempt,
    wait_exponential,
)

from patent_mcp.cache import PatentMetadata, SessionCache, SourceAttempt
from patent_mcp.fetchers.base import BasePatentSource, FetchResult
from patent_mcp.utils import now_iso

if TYPE_CHECKING:
    from patent_mcp.config import PatentConfig
    from patent_mcp.id_canon import CanonicalPatentId

log = logging.getLogger(__name__)

DEFAULT_USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36 patent-mcp-server/0.1"
)


def _client_kwargs(**kwargs: Any) -> dict[str, Any]:
    headers = dict(kwargs.pop("headers", {}))
    headers.setdefault("User-Agent", DEFAULT_USER_AGENT)
    return {**kwargs, "headers": headers}


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _is_retryable(exc: BaseException) -> bool:
    """Retry on 429/5xx HTTP errors and network timeouts."""
    if isinstance(exc, httpx.HTTPStatusError):
        return exc.response.status_code in (429, 500, 502, 503, 504)
    if isinstance(exc, (httpx.TimeoutException, httpx.ConnectError)):
        return True
    return False


def _retry_decorator():
    return retry(
        retry=retry_if_exception(_is_retryable),
        stop=stop_after_attempt(3),
        wait=wait_exponential(multiplier=1, min=1, max=8),
        reraise=True,
    )


# ---------------------------------------------------------------------------
# PPUBS session manager
# ---------------------------------------------------------------------------


class PpubsSessionManager:
    """Manages PPUBS session token (in-memory cache, TTL=30min)."""

    _SESSION_KEY = "PPUBS"
    _TTL_MINUTES = 30

    def __init__(self, config: "PatentConfig", session_cache: SessionCache) -> None:
        self._config = config
        self._cache = session_cache
        self._base = config.source_base_urls.get("USPTO", "https://ppubs.uspto.gov")

    async def get_session_token(self) -> str | None:
        cached = self._cache.get(self._SESSION_KEY)
        if cached:
            return cached
        token = await self._establish_session()
        if token:
            self._cache.set(self._SESSION_KEY, token, ttl_minutes=self._TTL_MINUTES)
        return token

    async def _establish_session(self) -> str | None:
        url = f"{self._base}/ppubs-api/v1/session"
        try:
            async with httpx.AsyncClient(
                **_client_kwargs(http2=True, timeout=30)
            ) as client:
                resp = await client.post(url, json={})
                resp.raise_for_status()
                data = resp.json()
                return (
                    data.get("session") or data.get("token") or data.get("accessToken")
                )
        except Exception as e:
            log.warning("PPUBS session establishment failed: %s", e)
            return None


# ---------------------------------------------------------------------------
# USPTO PPUBS
# ---------------------------------------------------------------------------


class PpubsSource(BasePatentSource):
    """USPTO PPUBS — US full-text patents, no auth required (session cookie)."""

    def __init__(
        self, config: "PatentConfig", session_cache: SessionCache | None = None
    ) -> None:
        super().__init__(config)
        self._session_mgr = PpubsSessionManager(config, session_cache or SessionCache())

    @property
    def source_name(self) -> str:
        return "USPTO"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset({"US"})

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        base = self._base_url("USPTO", "https://ppubs.uspto.gov")
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            token = await self._session_mgr.get_session_token()
            headers = {"Authorization": f"Bearer {token}"} if token else {}
            async with httpx.AsyncClient(
                **_client_kwargs(http2=True, timeout=30)
            ) as client:
                # Search for patent
                resp = await self._search_with_retry(
                    client, base, patent.number, headers
                )
                if not resp:
                    attempt.elapsed_ms = (time.monotonic() - start) * 1000
                    attempt.error = "not_found"
                    return FetchResult(source_attempt=attempt)

                guid, txt_content, meta = resp
                output_dir.mkdir(parents=True, exist_ok=True)

                txt_path: Path | None = None
                if txt_content:
                    txt_path = output_dir / f"{patent.canonical}.txt"
                    txt_path.write_text(txt_content, encoding="utf-8")

                # Attempt PDF download
                pdf_path: Path | None = None
                if guid:
                    pdf_path = await self._download_pdf(
                        client, base, guid, patent, output_dir, headers
                    )

                attempt.success = True
                attempt.elapsed_ms = (time.monotonic() - start) * 1000
                return FetchResult(
                    source_attempt=attempt,
                    pdf_path=pdf_path,
                    txt_path=txt_path,
                    metadata=meta,
                )
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            log.warning("PPUBS fetch failed for %s: %s", patent.canonical, e)
            return FetchResult(source_attempt=attempt)

    async def _search_with_retry(
        self,
        client: httpx.AsyncClient,
        base: str,
        number: str,
        headers: dict,
    ) -> tuple[str | None, str | None, PatentMetadata | None] | None:
        """Search PPUBS; return (guid, txt_content, metadata) or None."""
        url = f"{base}/ppubs-api/v1/patent"
        params = {"patentNumber": number}
        try:
            resp = await client.get(url, params=params, headers=headers)
            resp.raise_for_status()
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                return None
            raise

        data = resp.json()
        patents = data.get("patents") or data.get("results") or []
        if not patents:
            return None

        doc = patents[0]
        guid = doc.get("guid") or doc.get("documentId")
        txt_content = (
            doc.get("fullText") or doc.get("claims") or doc.get("abstract") or ""
        )
        meta = PatentMetadata(
            canonical_id=doc.get("patentNumber", ""),
            jurisdiction="US",
            doc_type="patent",
            title=doc.get("title"),
            abstract=doc.get("abstract"),
            inventors=doc.get("inventors", []),
            assignee=doc.get("assignee"),
            filing_date=doc.get("filingDate"),
            publication_date=doc.get("publicationDate"),
            grant_date=doc.get("grantDate"),
            fetched_at=now_iso(),
        )
        return guid, txt_content, meta

    async def _download_pdf(
        self,
        client: httpx.AsyncClient,
        base: str,
        guid: str,
        patent: "CanonicalPatentId",
        output_dir: Path,
        headers: dict,
    ) -> Path | None:
        url = f"{base}/ppubs-api/v1/download/{guid}"
        try:
            resp = await client.get(url, headers=headers)
            resp.raise_for_status()
            pdf_path = output_dir / f"{patent.canonical}.pdf"
            pdf_path.write_bytes(resp.content)
            return pdf_path
        except Exception as e:
            log.warning("PPUBS PDF download failed: %s", e)
            return None


# ---------------------------------------------------------------------------
# EPO OPS Token Manager
# ---------------------------------------------------------------------------


class EpoOpsTokenManager:
    """OAuth2 client credentials flow for EPO OPS."""

    _SESSION_KEY = "EPO_OPS"

    def __init__(
        self, config: "PatentConfig", session_cache: SessionCache | None = None
    ) -> None:
        self._config = config
        self._cache = session_cache or SessionCache()
        self._base = config.source_base_urls.get("EPO_OPS", "https://ops.epo.org")

    async def get_token(self) -> str | None:
        if not self._config.epo_client_id or not self._config.epo_client_secret:
            return None
        cached = self._cache.get(self._SESSION_KEY)
        if cached:
            return cached
        return await self._request_token()

    async def _request_token(self) -> str | None:
        url = f"{self._base}/3.2/auth/accesstoken"
        try:
            async with httpx.AsyncClient(**_client_kwargs(timeout=30)) as client:
                resp = await client.post(
                    url,
                    data={
                        "grant_type": "client_credentials",
                        "client_id": self._config.epo_client_id,
                        "client_secret": self._config.epo_client_secret,
                    },
                    headers={"Content-Type": "application/x-www-form-urlencoded"},
                )
                resp.raise_for_status()
                data = resp.json()
                token = data.get("access_token")
                expires_in = int(data.get("expires_in", 1800))
                if token:
                    expires_at = datetime.now(timezone.utc) + timedelta(
                        seconds=expires_in
                    )
                    self._cache.set_with_expiry(self._SESSION_KEY, token, expires_at)
                return token
        except httpx.HTTPStatusError as e:
            log.warning("EPO OPS auth failed: %s", e)
            return None
        except Exception as e:
            log.warning("EPO OPS token request failed: %s", e)
            return None


# ---------------------------------------------------------------------------
# EPO OPS Source
# ---------------------------------------------------------------------------


class EpoOpsSource(BasePatentSource):
    """EPO Open Patent Services — bibliographic data + PDF for 100+ offices."""

    def __init__(
        self, config: "PatentConfig", session_cache: SessionCache | None = None
    ) -> None:
        super().__init__(config)
        self._token_mgr = EpoOpsTokenManager(config, session_cache)

    @property
    def source_name(self) -> str:
        return "EPO_OPS"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset()  # supports all jurisdictions

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        base = self._base_url("EPO_OPS", "https://ops.epo.org")
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            token = await self._token_mgr.get_token()
            headers: dict = {}
            if token:
                headers["Authorization"] = f"Bearer {token}"
            else:
                # EPO OPS allows limited anonymous access
                pass

            async with httpx.AsyncClient(**_client_kwargs(timeout=30)) as client:
                meta = await self._fetch_biblio(client, base, patent, headers)
                if meta is None:
                    attempt.elapsed_ms = (time.monotonic() - start) * 1000
                    attempt.error = "not_found"
                    return FetchResult(source_attempt=attempt)

                output_dir.mkdir(parents=True, exist_ok=True)
                pdf_path = await self._fetch_pdf(
                    client, base, patent, output_dir, headers
                )

                attempt.success = True
                attempt.elapsed_ms = (time.monotonic() - start) * 1000
                return FetchResult(
                    source_attempt=attempt, pdf_path=pdf_path, metadata=meta
                )
        except httpx.HTTPStatusError as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            if e.response.status_code == 401:
                attempt.error = f"EPO OPS auth failed: {e}"
            else:
                attempt.error = str(e)
            return FetchResult(source_attempt=attempt)
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            log.warning("EPO OPS fetch failed for %s: %s", patent.canonical, e)
            return FetchResult(source_attempt=attempt)

    async def _fetch_biblio(
        self,
        client: httpx.AsyncClient,
        base: str,
        patent: "CanonicalPatentId",
        headers: dict,
    ) -> PatentMetadata | None:
        # Try epodoc format first
        pub_id = f"{patent.jurisdiction}.{patent.number}"
        url = f"{base}/3.2/rest-services/published-data/publication/epodoc/{pub_id}/biblio"
        try:
            resp = await client.get(url, headers=headers)
            if resp.status_code == 404:
                return None
            resp.raise_for_status()
            return self._parse_biblio_xml(resp.text, patent)
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 404:
                return None
            raise

    def _parse_biblio_xml(
        self, xml_text: str, patent: "CanonicalPatentId"
    ) -> PatentMetadata | None:
        """Parse EPO OPS biblio XML response."""
        try:
            root = ET.fromstring(xml_text)
            ns = {"ops": "http://ops.epo.org", "ep": "http://www.epo.org/exchange"}

            # Title
            title_el = root.find(".//ep:invention-title[@lang='en']", ns)
            title = title_el.text if title_el is not None else None

            # Inventors
            inventors = []
            for inv in root.findall(".//ep:inventor//ep:name", ns):
                if inv.text:
                    inventors.append(inv.text)

            # Assignee
            assignee_el = root.find(".//ep:applicant//ep:name", ns)
            assignee = assignee_el.text if assignee_el is not None else None

            # Dates
            def _get_date(tag: str) -> str | None:
                el = root.find(f".//{tag}", ns)
                return el.text if el is not None else None

            return PatentMetadata(
                canonical_id=patent.canonical,
                jurisdiction=patent.jurisdiction,
                doc_type=patent.doc_type,
                title=title,
                inventors=inventors,
                assignee=assignee,
                filing_date=_get_date("ep:filing-date"),
                publication_date=_get_date("ep:date-of-publication"),
                grant_date=_get_date("ep:date-of-grant"),
                fetched_at=now_iso(),
            )
        except ET.ParseError:
            return None

    async def _fetch_pdf(
        self,
        client: httpx.AsyncClient,
        base: str,
        patent: "CanonicalPatentId",
        output_dir: Path,
        headers: dict,
    ) -> Path | None:
        pub_id = f"{patent.jurisdiction}.{patent.number}"
        url = f"{base}/3.2/rest-services/published-data/publication/epodoc/{pub_id}/full-cycle"
        try:
            resp = await client.get(
                url, headers={**headers, "Accept": "application/pdf"}
            )
            resp.raise_for_status()
            if resp.headers.get("content-type", "").startswith("application/pdf"):
                pdf_path = output_dir / f"{patent.canonical}.pdf"
                pdf_path.write_bytes(resp.content)
                return pdf_path
        except Exception as e:
            log.debug("EPO OPS PDF download failed: %s", e)
        return None


# ---------------------------------------------------------------------------
# BigQuery Source
# ---------------------------------------------------------------------------


class BigQuerySource(BasePatentSource):
    """Google BigQuery patents-public-data — optional; degrades gracefully."""

    def __init__(self, config: "PatentConfig") -> None:
        super().__init__(config)
        self.available = False
        self._client: Any = None
        self._init_client()

    def _init_client(self) -> None:
        try:
            from google.cloud import bigquery  # type: ignore[import]
            import google.auth  # type: ignore[import]

            self._client = bigquery.Client()
            self.available = True
        except ImportError:
            log.debug(
                "google-cloud-bigquery not installed; BigQuery source unavailable"
            )
        except Exception as e:
            log.warning("BigQuery initialization failed: %s", e)

    @property
    def source_name(self) -> str:
        return "BigQuery"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset()  # handles all

    def _build_query(self, patent: "CanonicalPatentId") -> str:
        """Build BigQuery SQL for patent lookup."""
        # BigQuery format: CC-NNNNNN (e.g. US-7654321)
        pub_number = f"{patent.jurisdiction}-{patent.number}"
        return f"""
SELECT
  publication_number,
  title_localized,
  abstract_localized,
  inventor_harmonized,
  assignee_harmonized,
  filing_date,
  publication_date,
  grant_date
FROM `patents-public-data.patents.publications`
WHERE publication_number LIKE '{pub_number}%'
LIMIT 5
"""

    def _parse_row(self, row: Any, patent: "CanonicalPatentId") -> PatentMetadata:
        """Map BigQuery row to PatentMetadata."""
        titles = row.get("title_localized") or []
        title = next((t["text"] for t in titles if t.get("language") == "en"), None)
        if not title and titles:
            title = titles[0].get("text")

        abstracts = row.get("abstract_localized") or []
        abstract = next(
            (a["text"] for a in abstracts if a.get("language") == "en"), None
        )

        inventors = [i.get("name", "") for i in (row.get("inventor_harmonized") or [])]
        assignees = row.get("assignee_harmonized") or []
        assignee = assignees[0].get("name") if assignees else None

        def _parse_bq_date(d: Any) -> str | None:
            if not d:
                return None
            s = str(d)
            if len(s) == 8:  # YYYYMMDD
                return f"{s[:4]}-{s[4:6]}-{s[6:]}"
            return s

        return PatentMetadata(
            canonical_id=patent.canonical,
            jurisdiction=patent.jurisdiction,
            doc_type=patent.doc_type,
            title=title,
            abstract=abstract,
            inventors=inventors,
            assignee=assignee,
            filing_date=_parse_bq_date(row.get("filing_date")),
            publication_date=_parse_bq_date(row.get("publication_date")),
            grant_date=_parse_bq_date(row.get("grant_date")),
            fetched_at=now_iso(),
        )

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        if not self.available or self._client is None:
            attempt.error = "BigQuery not configured: no credentials"
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            return FetchResult(source_attempt=attempt)

        try:
            query = self._build_query(patent)
            rows = list(self._client.query(query).result())
            if not rows:
                attempt.error = "not_found"
                attempt.elapsed_ms = (time.monotonic() - start) * 1000
                return FetchResult(source_attempt=attempt)

            meta = self._parse_row(dict(rows[0]), patent)
            attempt.success = True
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            return FetchResult(source_attempt=attempt, metadata=meta)

        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            err_str = str(e)
            if "DeadlineExceeded" in err_str or "timeout" in err_str.lower():
                attempt.error = f"BigQuery timeout: {e}"
            elif "ResourceExhausted" in err_str or "quota" in err_str.lower():
                attempt.error = f"BigQuery quota exceeded: {e}"
                log.warning("BigQuery quota exceeded: %s", e)
            else:
                attempt.error = str(e)
            return FetchResult(source_attempt=attempt)


# ---------------------------------------------------------------------------
# Espacenet source
# ---------------------------------------------------------------------------


class EspacenetSource(BasePatentSource):
    """Espacenet — scrape HTML for metadata + PDF links."""

    @property
    def source_name(self) -> str:
        return "Espacenet"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset()

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        base = self._base_url("Espacenet", "https://worldwide.espacenet.com")
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            from bs4 import BeautifulSoup  # type: ignore[import]

            url = f"{base}/patent/{patent.canonical}"
            async with httpx.AsyncClient(
                **_client_kwargs(timeout=30, follow_redirects=True)
            ) as client:
                resp = await client.get(url, headers={"Accept-Language": "en"})
                if resp.status_code == 404:
                    attempt.error = "not_found"
                    attempt.elapsed_ms = (time.monotonic() - start) * 1000
                    return FetchResult(source_attempt=attempt)
                resp.raise_for_status()

            soup = BeautifulSoup(resp.text, "lxml")
            title_el = soup.find("h2", class_="title") or soup.find("h1")
            title = title_el.get_text(strip=True) if title_el else None

            # Look for PDF link
            pdf_url: str | None = None
            for a in soup.find_all("a", href=True):
                href = str(a["href"])
                if ".pdf" in href.lower() or "download" in href.lower():
                    pdf_url = href if href.startswith("http") else f"{base}{href}"
                    break

            meta = PatentMetadata(
                canonical_id=patent.canonical,
                jurisdiction=patent.jurisdiction,
                doc_type=patent.doc_type,
                title=title,
                fetched_at=now_iso(),
            )
            attempt.success = True
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            result = FetchResult(source_attempt=attempt, metadata=meta)
            if pdf_url:
                result.metadata = meta  # pdf_url stored in metadata for now
                result.source_attempt.metadata = {"pdf_url": pdf_url}
            return result
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            return FetchResult(source_attempt=attempt)


# ---------------------------------------------------------------------------
# WIPO PatentScope scraping
# ---------------------------------------------------------------------------


class WipoScrapeSource(BasePatentSource):
    """WIPO PatentScope — scrape for WO (PCT) patent data."""

    @property
    def source_name(self) -> str:
        return "WIPO_Scrape"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset({"WO"})

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        base = self._base_url("WIPO_Scrape", "https://patentscope.wipo.int")
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            from bs4 import BeautifulSoup  # type: ignore[import]

            # PCT number format: WO/YEAR/SERIAL → extract year and serial
            number = patent.number  # e.g. "2024123456"
            if len(number) >= 10:
                year = number[:4]
                serial = number[4:]
                wo_id = f"WO/{year}/{serial}"
            else:
                wo_id = patent.canonical
            url = f"{base}/search/en/detail.jsf?docId={wo_id}"
            async with httpx.AsyncClient(
                **_client_kwargs(timeout=30, follow_redirects=True)
            ) as client:
                resp = await client.get(url)
                if resp.status_code == 404:
                    attempt.error = "not_found"
                    attempt.elapsed_ms = (time.monotonic() - start) * 1000
                    return FetchResult(source_attempt=attempt)
                resp.raise_for_status()

            soup = BeautifulSoup(resp.text, "lxml")
            title_el = soup.find("span", id="appTitleId") or soup.find("title")
            title = title_el.get_text(strip=True) if title_el else None

            meta = PatentMetadata(
                canonical_id=patent.canonical,
                jurisdiction="WO",
                doc_type="application",
                title=title,
                fetched_at=now_iso(),
            )
            attempt.success = True
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            return FetchResult(source_attempt=attempt, metadata=meta)
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            return FetchResult(source_attempt=attempt)


# ---------------------------------------------------------------------------
# IP Australia (AusPat)
# ---------------------------------------------------------------------------


class IpAustraliaSource(BasePatentSource):
    """IP Australia AusPat REST API."""

    @property
    def source_name(self) -> str:
        return "IP_Australia"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset({"AU"})

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        base = self._base_url("IP_Australia", "https://pericles.ipaustralia.gov.au")
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            url = f"{base}/ols/auspat/api/v1/applications/{patent.number}"
            async with httpx.AsyncClient(**_client_kwargs(timeout=30)) as client:
                resp = await client.get(url, headers={"Accept": "application/json"})
                if resp.status_code == 404:
                    attempt.error = "not_found"
                    attempt.elapsed_ms = (time.monotonic() - start) * 1000
                    return FetchResult(source_attempt=attempt)
                resp.raise_for_status()

            data = resp.json()
            meta = PatentMetadata(
                canonical_id=patent.canonical,
                jurisdiction="AU",
                doc_type=patent.doc_type,
                title=data.get("title"),
                inventors=[i.get("name", "") for i in data.get("inventors", [])],
                assignee=data.get("applicant"),
                filing_date=data.get("filingDate"),
                publication_date=data.get("publicationDate"),
                grant_date=data.get("grantDate"),
                fetched_at=now_iso(),
            )
            attempt.success = True
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            return FetchResult(source_attempt=attempt, metadata=meta)
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            return FetchResult(source_attempt=attempt)


# ---------------------------------------------------------------------------
# CIPO (Canada)
# ---------------------------------------------------------------------------


class CipoScrapeSource(BasePatentSource):
    """CIPO — scrape Canadian patent database."""

    @property
    def source_name(self) -> str:
        return "CIPO"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset({"CA"})

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        start = time.monotonic()
        base = self._base_url(
            "CIPO", "https://patents.google.com"
        )  # fallback to google
        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            from bs4 import BeautifulSoup  # type: ignore[import]

            url = f"{base}/patent/{patent.canonical}"
            async with httpx.AsyncClient(
                **_client_kwargs(timeout=30, follow_redirects=True)
            ) as client:
                resp = await client.get(url)
                if resp.status_code == 404:
                    attempt.error = "not_found"
                    attempt.elapsed_ms = (time.monotonic() - start) * 1000
                    return FetchResult(source_attempt=attempt)
                resp.raise_for_status()

            soup = BeautifulSoup(resp.text, "lxml")
            title_el = soup.find("h1") or soup.find("title")
            title = title_el.get_text(strip=True) if title_el else None
            meta = PatentMetadata(
                canonical_id=patent.canonical,
                jurisdiction="CA",
                doc_type=patent.doc_type,
                title=title,
                fetched_at=now_iso(),
            )
            attempt.success = True
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            return FetchResult(source_attempt=attempt, metadata=meta)
        except Exception as e:
            attempt.elapsed_ms = (time.monotonic() - start) * 1000
            attempt.error = str(e)
            return FetchResult(source_attempt=attempt)


# ---------------------------------------------------------------------------
# Google Patents (browser scraper wrapper)
# ---------------------------------------------------------------------------


class GooglePatentsSource(BasePatentSource):
    """Google Patents — wraps the Playwright-based browser scraper."""

    @property
    def source_name(self) -> str:
        return "Google_Patents"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset()  # handles all jurisdictions

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        import asyncio
        from patent_mcp.cache import PatentMetadata, SourceAttempt
        from patent_mcp.scrapers import google_patents as _gp

        attempt = SourceAttempt(source=self.source_name, success=False, elapsed_ms=0.0)
        try:
            loop = asyncio.get_running_loop()
            browser_result = await loop.run_in_executor(
                None, _gp.fetch, patent.canonical, output_dir
            )
        except Exception as e:
            attempt.error = str(e)
            log.warning("Google Patents fetch failed for %s: %s", patent.canonical, e)
            return FetchResult(source_attempt=attempt)

        attempt.success = browser_result.success
        attempt.elapsed_ms = browser_result.elapsed_ms
        if browser_result.error:
            attempt.error = browser_result.error

        if not browser_result.success:
            return FetchResult(source_attempt=attempt)

        meta = PatentMetadata(
            canonical_id=patent.canonical,
            jurisdiction=patent.jurisdiction,
            doc_type=patent.doc_type,
            title=browser_result.title,
            abstract=browser_result.abstract,
            inventors=browser_result.inventors,
            assignee=browser_result.assignee,
            filing_date=browser_result.filing_date,
            publication_date=browser_result.publication_date,
            fetched_at=now_iso(),
        )
        pdf_path = Path(browser_result.pdf_path) if browser_result.pdf_path else None
        txt_path = Path(browser_result.txt_path) if browser_result.txt_path else None
        return FetchResult(
            source_attempt=attempt,
            pdf_path=pdf_path,
            txt_path=txt_path,
            metadata=meta,
        )


# ---------------------------------------------------------------------------
# PatentsView stub (deprecated)
# ---------------------------------------------------------------------------


class PatentsViewStubSource(BasePatentSource):
    """PatentsView was shut down March 20, 2026. Returns helpful error."""

    @property
    def source_name(self) -> str:
        return "PatentsView"

    @property
    def supported_jurisdictions(self) -> frozenset[str]:
        return frozenset({"US"})

    async def fetch(self, patent: "CanonicalPatentId", output_dir: Path) -> FetchResult:
        attempt = SourceAttempt(
            source=self.source_name,
            success=False,
            elapsed_ms=0.0,
            error="PatentsView API was shut down March 20, 2026. Use USPTO ODP instead.",
        )
        return FetchResult(source_attempt=attempt)
