"""Patent search backends: async HTTP clients that query patent databases.

Three backends are provided:
- SerpApiGooglePatentsBackend  — Google Patents via SerpAPI
- UsptoTextSearchBackend       — USPTO PPUBS full-text search
- EpoOpsSearchBackend          — EPO Open Patent Services (OPS)
"""
from __future__ import annotations

import logging
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from typing import Any

import httpx

from patent_mcp.search.session_manager import PatentHit  # canonical definition

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Helper
# ---------------------------------------------------------------------------

def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


# ---------------------------------------------------------------------------
# Backend 1: SerpAPI Google Patents
# ---------------------------------------------------------------------------

class SerpApiGooglePatentsBackend:
    """Search Google Patents via SerpAPI's google_patents engine."""

    SERPAPI_URL = "https://serpapi.com/search"

    def __init__(self, api_key: str, base_url: str | None = None) -> None:
        self._api_key = api_key
        self._base_url = base_url or self.SERPAPI_URL

    async def search(
        self,
        query: str,
        date_from: str | None = None,    # "YYYY-MM-DD"
        date_to: str | None = None,      # "YYYY-MM-DD"
        assignee: str | None = None,
        inventor: str | None = None,
        patent_type: str | None = None,  # "PATENT" or "APPLICATION"
        max_results: int = 25,
    ) -> list[PatentHit]:
        """Search Google Patents via SerpAPI.

        Returns a list of PatentHit objects; returns empty list on any error.
        """
        params: dict[str, Any] = {
            "engine": "google_patents",
            "q": query,
            "api_key": self._api_key,
            "num": max_results,
        }

        # Date filters: SerpAPI expects YYYY/MM/DD
        if date_from:
            params["after_priority_date"] = date_from.replace("-", "/")
        if date_to:
            params["before_priority_date"] = date_to.replace("-", "/")
        if assignee:
            params["assignee"] = assignee
        if inventor:
            params["inventor"] = inventor
        if patent_type:
            params["type"] = patent_type

        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.get(self._base_url, params=params)
                resp.raise_for_status()
                data = resp.json()
        except httpx.HTTPStatusError as exc:
            log.warning(
                "SerpAPI Google Patents HTTP error %s: %s",
                exc.response.status_code,
                exc,
            )
            return []
        except Exception as exc:
            log.warning("SerpAPI Google Patents request failed: %s", exc)
            return []

        organic = data.get("organic_results") or []
        hits: list[PatentHit] = []
        for item in organic:
            hit = self._map_result(item)
            if hit is not None:
                hits.append(hit)
        return hits

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _map_result(self, item: dict[str, Any]) -> PatentHit | None:
        """Map a single SerpAPI organic_result entry to a PatentHit."""
        patent_id = (
            item.get("patent_id")
            or item.get("result_id")
            or item.get("id")
        )
        if not patent_id:
            return None

        # Prefer grant_date > filing_date > priority_date for the date field
        date = (
            item.get("grant_date")
            or item.get("filing_date")
            or item.get("priority_date")
        )

        inventors_raw = item.get("inventor") or []
        if isinstance(inventors_raw, str):
            inventors_raw = [inventors_raw]

        # PDF link as the canonical URL
        url = item.get("pdf") or item.get("link")

        return PatentHit(
            patent_id=patent_id,
            title=item.get("title"),
            date=date,
            assignee=item.get("assignee"),
            inventors=list(inventors_raw),
            abstract=item.get("snippet") or item.get("abstract"),
            source="SerpAPI_Google_Patents",
            relevance="unknown",
            url=url,
        )


# ---------------------------------------------------------------------------
# Backend 2: USPTO PPUBS Text Search
# ---------------------------------------------------------------------------

class UsptoTextSearchBackend:
    """Search USPTO PPUBS full-text search API."""

    PPUBS_BASE = "https://ppubs.uspto.gov/ppubs-api/v1"

    def __init__(self, base_url: str | None = None) -> None:
        self._base = (base_url or self.PPUBS_BASE).rstrip("/")

    async def search(
        self,
        query: str,           # Boolean query like "TTL/wireless AND ACLM/charging"
        date_from: str | None = None,   # "YYYYMMDD"
        date_to: str | None = None,     # "YYYYMMDD"
        max_results: int = 25,
    ) -> list[PatentHit]:
        """Search USPTO PPUBS full-text search API.

        Returns a list of PatentHit objects; returns empty list on any error.
        """
        body: dict[str, Any] = {
            "query": query,
            "sources": ["US-PGPUB", "USPAT"],
            "hits": max_results,
            "start": 0,
        }
        if date_from or date_to:
            body["dateRangeField"] = "applicationDate"
        if date_from:
            body["startDate"] = date_from
        if date_to:
            body["endDate"] = date_to

        url = f"{self._base}/query"
        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.post(url, json=body)
                resp.raise_for_status()
                data = resp.json()
        except httpx.HTTPStatusError as exc:
            log.warning(
                "USPTO PPUBS text search HTTP error %s: %s",
                exc.response.status_code,
                exc,
            )
            return []
        except Exception as exc:
            log.warning("USPTO PPUBS text search request failed: %s", exc)
            return []

        patents = data.get("patents") or data.get("results") or []
        hits: list[PatentHit] = []
        for doc in patents:
            hit = self._map_doc(doc)
            if hit is not None:
                hits.append(hit)
        return hits

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _map_doc(self, doc: dict[str, Any]) -> PatentHit | None:
        """Map a single PPUBS patent document to a PatentHit."""
        patent_id = (
            doc.get("patentNumber")
            or doc.get("patent_number")
            or doc.get("documentId")
        )
        if not patent_id:
            return None

        date = (
            doc.get("grantDate")
            or doc.get("grant_date")
            or doc.get("publicationDate")
            or doc.get("publication_date")
            or doc.get("filingDate")
            or doc.get("filing_date")
        )

        inventors_raw = doc.get("inventors") or []
        if isinstance(inventors_raw, str):
            inventors_raw = [inventors_raw]

        return PatentHit(
            patent_id=patent_id,
            title=doc.get("title"),
            date=date,
            assignee=doc.get("assignee"),
            inventors=list(inventors_raw),
            abstract=doc.get("abstract"),
            source="USPTO_PPUBS",
            relevance="unknown",
        )


# ---------------------------------------------------------------------------
# Backend 3: EPO OPS Search
# ---------------------------------------------------------------------------

class EpoOpsSearchBackend:
    """Search EPO Open Patent Services (OPS) API."""

    OPS_BASE = "https://ops.epo.org/3.2/rest-services"
    _AUTH_URL = "https://ops.epo.org/3.2/auth/accesstoken"

    def __init__(
        self,
        client_id: str | None = None,
        client_secret: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self._client_id = client_id
        self._client_secret = client_secret
        self._base = (base_url or self.OPS_BASE).rstrip("/")
        # Determine auth URL from base_url if provided (replace last path component)
        if base_url:
            # Strip rest-services suffix to get root, then append auth path
            root = base_url.rstrip("/")
            if root.endswith("/rest-services"):
                root = root[: -len("/rest-services")]
            self._auth_url = f"{root}/auth/accesstoken"
        else:
            self._auth_url = self._AUTH_URL

        self._token: str | None = None
        self._token_expires_at: float = 0.0  # monotonic timestamp

    # ------------------------------------------------------------------
    # OAuth
    # ------------------------------------------------------------------

    async def get_oauth_token(self) -> str | None:
        """Obtain (or return cached) OAuth2 bearer token.

        Returns None if no credentials are configured or if auth fails.
        """
        if not self._client_id or not self._client_secret:
            return None

        # Return cached token if still valid (with 60-second buffer)
        if self._token and time.monotonic() < self._token_expires_at - 60:
            return self._token

        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.post(
                    self._auth_url,
                    data={
                        "grant_type": "client_credentials",
                        "client_id": self._client_id,
                        "client_secret": self._client_secret,
                    },
                    headers={"Content-Type": "application/x-www-form-urlencoded"},
                )
                resp.raise_for_status()
                data = resp.json()
                token = data.get("access_token")
                expires_in = int(data.get("expires_in", 1800))
                if token:
                    self._token = token
                    self._token_expires_at = time.monotonic() + expires_in
                return token
        except httpx.HTTPStatusError as exc:
            log.warning("EPO OPS OAuth failed: %s", exc)
            return None
        except Exception as exc:
            log.warning("EPO OPS OAuth request failed: %s", exc)
            return None

    # ------------------------------------------------------------------
    # Search
    # ------------------------------------------------------------------

    async def search(
        self,
        query: str,           # CQL query like "ti=wireless AND cl=H02J50"
        date_from: str | None = None,
        date_to: str | None = None,
        max_results: int = 25,
    ) -> list[PatentHit]:
        """Search EPO OPS published-data search endpoint.

        Returns a list of PatentHit objects; returns empty list on any error.
        """
        cql = query
        if date_from or date_to:
            date_clauses = []
            if date_from:
                date_clauses.append(f"pd>={date_from.replace('-', '')}")
            if date_to:
                date_clauses.append(f"pd<={date_to.replace('-', '')}")
            cql = f"({cql}) AND {' AND '.join(date_clauses)}"

        token = await self.get_oauth_token()
        headers: dict[str, str] = {"Accept": "application/json"}
        if token:
            headers["Authorization"] = f"Bearer {token}"

        url = f"{self._base}/published-data/search"
        params = {
            "q": cql,
            "Range": f"1-{max_results}",
        }

        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.get(url, params=params, headers=headers)
                resp.raise_for_status()
                # EPO OPS may return JSON or XML depending on Accept header support
                content_type = resp.headers.get("content-type", "")
                if "json" in content_type:
                    return self._parse_json_response(resp.json())
                else:
                    return self._parse_xml_response(resp.text)
        except httpx.HTTPStatusError as exc:
            log.warning(
                "EPO OPS search HTTP error %s: %s",
                exc.response.status_code,
                exc,
            )
            return []
        except Exception as exc:
            log.warning("EPO OPS search request failed: %s", exc)
            return []

    # ------------------------------------------------------------------
    # Classification search
    # ------------------------------------------------------------------

    async def search_by_classification(
        self,
        cpc_code: str,              # like "H02J50" or "H02J50/10"
        include_subclasses: bool = True,
        date_from: str | None = None,
        date_to: str | None = None,
        max_results: int = 25,
    ) -> list[PatentHit]:
        """Search EPO OPS by CPC classification code.

        Builds a CQL query and delegates to self.search().
        """
        if include_subclasses:
            # Wildcard for all subclasses: H02J50/* matches H02J50/10, H02J50/20, etc.
            code_expr = f"cpc={cpc_code}/*"
        else:
            code_expr = f"cpc={cpc_code}"

        return await self.search(
            code_expr,
            date_from=date_from,
            date_to=date_to,
            max_results=max_results,
        )

    # ------------------------------------------------------------------
    # Citations
    # ------------------------------------------------------------------

    async def get_citations(
        self,
        patent_id: str,
        direction: str = "backward",  # "forward" or "backward"
    ) -> list[str]:
        """Retrieve patent citations via EPO OPS.

        Backward: patents cited by this one (prior art it cites).
        Forward: patents that cite this one (harder; approximated via search).

        Returns a list of patent ID strings; returns empty list on any error.
        """
        token = await self.get_oauth_token()
        headers: dict[str, str] = {"Accept": "application/json"}
        if token:
            headers["Authorization"] = f"Bearer {token}"

        if direction == "backward":
            url = f"{self._base}/published-data/citation/epodoc/{patent_id}"
            try:
                async with httpx.AsyncClient(timeout=30) as client:
                    resp = await client.get(url, headers=headers)
                    resp.raise_for_status()
                    content_type = resp.headers.get("content-type", "")
                    if "json" in content_type:
                        return self._extract_ids_from_json(resp.json())
                    else:
                        return self._extract_ids_from_citation_xml(resp.text)
            except Exception as exc:
                log.warning("EPO OPS citation fetch failed for %s: %s", patent_id, exc)
                return []
        else:
            # Forward citations: search for patents that cite this one
            # EPO OPS doesn't have a direct forward-citation endpoint; use CQL
            cql = f"ct={patent_id}"
            hits = await self.search(cql, max_results=100)
            return [h.patent_id for h in hits]

    # ------------------------------------------------------------------
    # Patent family
    # ------------------------------------------------------------------

    async def get_family(self, patent_id: str) -> list[dict[str, Any]]:
        """Retrieve patent family members via EPO OPS.

        Returns a list of dicts with keys: patent_id, country, doc_type, date.
        Returns empty list on any error.
        """
        token = await self.get_oauth_token()
        headers: dict[str, str] = {"Accept": "application/json"}
        if token:
            headers["Authorization"] = f"Bearer {token}"

        url = f"{self._base}/family/publication/epodoc/{patent_id}"
        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.get(url, headers=headers)
                resp.raise_for_status()
                content_type = resp.headers.get("content-type", "")
                if "json" in content_type:
                    return self._parse_family_json(resp.json())
                else:
                    return self._parse_family_xml(resp.text)
        except Exception as exc:
            log.warning("EPO OPS family fetch failed for %s: %s", patent_id, exc)
            return []

    # ------------------------------------------------------------------
    # Response parsers — JSON
    # ------------------------------------------------------------------

    def _parse_json_response(self, data: dict[str, Any]) -> list[PatentHit]:
        """Parse EPO OPS JSON search response into PatentHit list."""
        hits: list[PatentHit] = []
        try:
            # Navigate the nested EPO OPS JSON structure
            ops_data = (
                data
                .get("ops:world-patent-data", data)
                .get("ops:biblio-search", data)
                .get("ops:search-result", {})
            )
            exchange_docs = ops_data.get("exchange-documents", [])
            if isinstance(exchange_docs, dict):
                exchange_docs = [exchange_docs]

            for doc_wrapper in exchange_docs:
                doc = doc_wrapper.get("exchange-document", doc_wrapper)
                hit = self._map_ops_json_doc(doc)
                if hit is not None:
                    hits.append(hit)
        except Exception as exc:
            log.warning("EPO OPS JSON parse error: %s", exc)
        return hits

    def _map_ops_json_doc(self, doc: dict[str, Any]) -> PatentHit | None:
        """Map a single EPO OPS JSON exchange-document to a PatentHit."""
        # Patent ID from doc attributes or doc-id
        patent_id = doc.get("@doc-number") or doc.get("doc-number")
        country = doc.get("@country") or doc.get("country", "")
        kind = doc.get("@kind") or doc.get("kind", "")
        if patent_id:
            patent_id = f"{country}{patent_id}{kind}".strip()
        if not patent_id:
            return None

        biblio = doc.get("bibliographic-data", {})

        # Title
        title_data = biblio.get("invention-title", [])
        if isinstance(title_data, dict):
            title_data = [title_data]
        title: str | None = None
        for t in title_data:
            if isinstance(t, dict):
                lang = t.get("@lang", "")
                val = t.get("$") or t.get("#text") or t.get("text")
                if lang == "en" and val:
                    title = val
                    break
                if val and title is None:
                    title = val

        # Date
        pub_refs = biblio.get("publication-reference", {})
        if isinstance(pub_refs, dict):
            pub_refs = [pub_refs]
        date: str | None = None
        for ref in pub_refs:
            doc_id_list = ref.get("document-id", [])
            if isinstance(doc_id_list, dict):
                doc_id_list = [doc_id_list]
            for did in doc_id_list:
                d = did.get("date", {})
                val = d.get("$") or d.get("#text") if isinstance(d, dict) else str(d)
                if val:
                    date = val
                    break

        # Inventors
        inventors: list[str] = []
        parties = biblio.get("parties", {})
        inv_section = parties.get("inventors", {}).get("inventor", [])
        if isinstance(inv_section, dict):
            inv_section = [inv_section]
        for inv in inv_section:
            name_data = inv.get("inventor-name", {}).get("name", {})
            name = (
                name_data.get("$") or name_data.get("#text")
                if isinstance(name_data, dict)
                else str(name_data)
            )
            if name:
                inventors.append(name)

        # Assignee
        app_section = parties.get("applicants", {}).get("applicant", [])
        if isinstance(app_section, dict):
            app_section = [app_section]
        assignee: str | None = None
        for app in app_section:
            name_data = app.get("applicant-name", {}).get("name", {})
            val = (
                name_data.get("$") or name_data.get("#text")
                if isinstance(name_data, dict)
                else str(name_data)
            )
            if val:
                assignee = val
                break

        return PatentHit(
            patent_id=patent_id,
            title=title,
            date=date,
            assignee=assignee,
            inventors=inventors,
            source="EPO_OPS",
            relevance="unknown",
        )

    def _extract_ids_from_json(self, data: dict[str, Any]) -> list[str]:
        """Extract patent IDs from EPO OPS citation JSON response."""
        ids: list[str] = []
        try:
            world_data = data.get("ops:world-patent-data", data)
            citation_list = world_data.get("ops:citation", [])
            if isinstance(citation_list, dict):
                citation_list = [citation_list]
            for c in citation_list:
                doc_id = c.get("patcit", {}).get("document-id", {})
                num = doc_id.get("doc-number", {})
                val = num.get("$") or num.get("#text") if isinstance(num, dict) else str(num)
                country = doc_id.get("country", {})
                cc = country.get("$") if isinstance(country, dict) else str(country)
                if val:
                    ids.append(f"{cc or ''}{val}")
        except Exception as exc:
            log.warning("EPO OPS citation JSON parse error: %s", exc)
        return ids

    def _parse_family_json(self, data: dict[str, Any]) -> list[dict[str, Any]]:
        """Parse EPO OPS family JSON response into a list of member dicts."""
        members: list[dict[str, Any]] = []
        try:
            world_data = data.get("ops:world-patent-data", data)
            family_data = world_data.get("ops:patent-family", {})
            family_members = family_data.get("ops:family-member", [])
            if isinstance(family_members, dict):
                family_members = [family_members]
            for m in family_members:
                pub_refs = m.get("publication-reference", {})
                if isinstance(pub_refs, dict):
                    pub_refs = [pub_refs]
                for ref in pub_refs:
                    doc_ids = ref.get("document-id", [])
                    if isinstance(doc_ids, dict):
                        doc_ids = [doc_ids]
                    for did in doc_ids:
                        country = did.get("country", {})
                        cc = country.get("$") if isinstance(country, dict) else str(country)
                        num = did.get("doc-number", {})
                        n = num.get("$") if isinstance(num, dict) else str(num)
                        kind = did.get("kind", {})
                        k = kind.get("$") if isinstance(kind, dict) else str(kind)
                        date_obj = did.get("date", {})
                        d = date_obj.get("$") if isinstance(date_obj, dict) else str(date_obj)
                        if n:
                            members.append({
                                "patent_id": f"{cc or ''}{n}{k or ''}",
                                "country": cc or "",
                                "doc_type": k or "",
                                "date": d or "",
                            })
        except Exception as exc:
            log.warning("EPO OPS family JSON parse error: %s", exc)
        return members

    # ------------------------------------------------------------------
    # Response parsers — XML
    # ------------------------------------------------------------------

    def _parse_xml_response(self, xml_text: str) -> list[PatentHit]:
        """Parse EPO OPS XML search response into PatentHit list."""
        hits: list[PatentHit] = []
        try:
            root = ET.fromstring(xml_text)
            ns = {
                "ops": "http://ops.epo.org",
                "ep": "http://www.epo.org/exchange",
            }
            for doc in root.findall(".//ep:exchange-document", ns):
                hit = self._map_ops_xml_doc(doc, ns)
                if hit is not None:
                    hits.append(hit)
        except ET.ParseError as exc:
            log.warning("EPO OPS XML parse error: %s", exc)
        return hits

    def _map_ops_xml_doc(
        self, doc: ET.Element, ns: dict[str, str]
    ) -> PatentHit | None:
        """Map a single EPO OPS XML exchange-document element to a PatentHit."""
        country = doc.get("country", "")
        doc_number = doc.get("doc-number", "")
        kind = doc.get("kind", "")
        if not doc_number:
            return None
        patent_id = f"{country}{doc_number}{kind}"

        # Title
        title_el = doc.find(".//ep:invention-title[@lang='en']", ns)
        if title_el is None:
            title_el = doc.find(".//ep:invention-title", ns)
        title = title_el.text.strip() if title_el is not None and title_el.text else None

        # Date
        date_el = doc.find(".//ep:date-of-publication", ns)
        if date_el is None:
            date_el = doc.find(".//ep:date", ns)
        date = date_el.text.strip() if date_el is not None and date_el.text else None

        # Inventors
        inventors = []
        for inv in doc.findall(".//ep:inventor//ep:name", ns):
            if inv.text:
                inventors.append(inv.text.strip())

        # Assignee (applicant)
        assignee_el = doc.find(".//ep:applicant//ep:name", ns)
        assignee = (
            assignee_el.text.strip()
            if assignee_el is not None and assignee_el.text
            else None
        )

        return PatentHit(
            patent_id=patent_id,
            title=title,
            date=date,
            assignee=assignee,
            inventors=inventors,
            source="EPO_OPS",
            relevance="unknown",
        )

    def _extract_ids_from_citation_xml(self, xml_text: str) -> list[str]:
        """Extract patent IDs from EPO OPS citation XML response."""
        ids: list[str] = []
        try:
            root = ET.fromstring(xml_text)
            ns = {
                "ops": "http://ops.epo.org",
                "ep": "http://www.epo.org/exchange",
            }
            for doc in root.findall(".//ep:document-id", ns):
                country_el = doc.find("ep:country", ns)
                num_el = doc.find("ep:doc-number", ns)
                if num_el is not None and num_el.text:
                    cc = country_el.text.strip() if country_el is not None and country_el.text else ""
                    ids.append(f"{cc}{num_el.text.strip()}")
        except ET.ParseError as exc:
            log.warning("EPO OPS citation XML parse error: %s", exc)
        return ids

    def _parse_family_xml(self, xml_text: str) -> list[dict[str, Any]]:
        """Parse EPO OPS family XML response into a list of member dicts."""
        members: list[dict[str, Any]] = []
        try:
            root = ET.fromstring(xml_text)
            ns = {
                "ops": "http://ops.epo.org",
                "ep": "http://www.epo.org/exchange",
            }
            for member in root.findall(".//ops:family-member", ns):
                for doc in member.findall(".//ep:document-id", ns):
                    country_el = doc.find("ep:country", ns)
                    num_el = doc.find("ep:doc-number", ns)
                    kind_el = doc.find("ep:kind", ns)
                    date_el = doc.find("ep:date", ns)
                    if num_el is not None and num_el.text:
                        cc = country_el.text.strip() if country_el is not None and country_el.text else ""
                        num = num_el.text.strip()
                        kind = kind_el.text.strip() if kind_el is not None and kind_el.text else ""
                        date = date_el.text.strip() if date_el is not None and date_el.text else ""
                        members.append({
                            "patent_id": f"{cc}{num}{kind}",
                            "country": cc,
                            "doc_type": kind,
                            "date": date,
                        })
        except ET.ParseError as exc:
            log.warning("EPO OPS family XML parse error: %s", exc)
        return members
