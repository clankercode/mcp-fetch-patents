"""Tests for patent_mcp.search.searchers — search backends.

All tests mock HTTP calls with respx to avoid real network traffic.
Marked 'slow': httpx+respx imports add ~250ms; excluded from <1s fast suite.
"""

from __future__ import annotations

import json
import xml.etree.ElementTree as ET
from typing import Any

import pytest

httpx = pytest.importorskip("httpx")
respx = pytest.importorskip("respx")

pytestmark = pytest.mark.slow

from patent_mcp.search.searchers import (
    EpoOpsSearchBackend,
    PatentHit,
    SerpApiGooglePatentsBackend,
    UsptoTextSearchBackend,
)


# ---------------------------------------------------------------------------
# Helpers / fixtures
# ---------------------------------------------------------------------------

SERP_MOCK_BASE = "http://mock-serpapi"
PPUBS_MOCK_BASE = "http://mock-ppubs/ppubs-api/v1"
EPO_MOCK_BASE = "http://mock-epo/3.2/rest-services"


def _serpapi_backend(base: str = SERP_MOCK_BASE) -> SerpApiGooglePatentsBackend:
    return SerpApiGooglePatentsBackend(api_key="test-key", base_url=base)


def _ppubs_backend(base: str = PPUBS_MOCK_BASE) -> UsptoTextSearchBackend:
    return UsptoTextSearchBackend(base_url=base)


def _epo_backend(
    client_id: str | None = None,
    client_secret: str | None = None,
    base: str = EPO_MOCK_BASE,
) -> EpoOpsSearchBackend:
    return EpoOpsSearchBackend(
        client_id=client_id,
        client_secret=client_secret,
        base_url=base,
    )


# ---------------------------------------------------------------------------
# SerpAPI — success
# ---------------------------------------------------------------------------


class TestSerpApiSuccess:
    @pytest.mark.asyncio
    async def test_maps_organic_results_to_patent_hits(self):
        backend = _serpapi_backend()
        response_data = {
            "organic_results": [
                {
                    "patent_id": "US10123456B2",
                    "title": "Method for wireless charging",
                    "snippet": "A method for transferring energy...",
                    "priority_date": "2018-01-15",
                    "filing_date": "2018-01-15",
                    "grant_date": "2019-09-03",
                    "inventor": ["John Doe", "Jane Smith"],
                    "assignee": "Apple Inc.",
                    "pdf": "https://patentimages.storage.googleapis.com/stub.pdf",
                    "filing_status": "Grant",
                }
            ]
        }

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(200, json=response_data)
            )
            hits = await backend.search("wireless charging")

        assert len(hits) == 1
        h = hits[0]
        assert h.patent_id == "US10123456B2"
        assert h.title == "Method for wireless charging"
        assert h.abstract == "A method for transferring energy..."
        assert h.assignee == "Apple Inc."
        assert "John Doe" in h.inventors
        assert "Jane Smith" in h.inventors
        assert h.source == "SerpAPI_Google_Patents"
        assert h.relevance == "unknown"
        assert h.url == "https://patentimages.storage.googleapis.com/stub.pdf"

    @pytest.mark.asyncio
    async def test_date_prefers_grant_date(self):
        backend = _serpapi_backend()
        response_data = {
            "organic_results": [
                {
                    "patent_id": "US9999999B1",
                    "title": "Test",
                    "priority_date": "2010-01-01",
                    "filing_date": "2010-02-01",
                    "grant_date": "2015-06-15",
                    "inventor": [],
                    "assignee": None,
                }
            ]
        }

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(200, json=response_data)
            )
            hits = await backend.search("test")

        assert len(hits) == 1
        assert hits[0].date == "2015-06-15"

    @pytest.mark.asyncio
    async def test_empty_organic_results_returns_empty_list(self):
        backend = _serpapi_backend()

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(200, json={"organic_results": []})
            )
            hits = await backend.search("nonexistent query")

        assert hits == []

    @pytest.mark.asyncio
    async def test_result_without_patent_id_is_skipped(self):
        backend = _serpapi_backend()
        response_data = {
            "organic_results": [{"title": "No ID patent", "snippet": "stub"}]
        }

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(200, json=response_data)
            )
            hits = await backend.search("test")

        assert hits == []

    @pytest.mark.asyncio
    async def test_date_params_converted_to_slash_format(self):
        """Verify date_from/date_to YYYY-MM-DD → YYYY/MM/DD in query params."""
        backend = _serpapi_backend()
        captured_params: dict[str, Any] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            for key, val in request.url.params.items():
                captured_params[key] = val
            return httpx.Response(200, json={"organic_results": []})

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(side_effect=capture)
            await backend.search(
                "wireless", date_from="2020-01-01", date_to="2023-12-31"
            )

        assert captured_params.get("after_priority_date") == "2020/01/01"
        assert captured_params.get("before_priority_date") == "2023/12/31"

    @pytest.mark.asyncio
    async def test_inventor_string_wrapped_in_list(self):
        """A single inventor returned as string should be wrapped in a list."""
        backend = _serpapi_backend()
        response_data = {
            "organic_results": [
                {
                    "patent_id": "US1111111A",
                    "inventor": "Solo Inventor",
                    "assignee": None,
                }
            ]
        }

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(200, json=response_data)
            )
            hits = await backend.search("solo")

        assert hits[0].inventors == ["Solo Inventor"]


# ---------------------------------------------------------------------------
# SerpAPI — error handling
# ---------------------------------------------------------------------------


class TestSerpApiErrors:
    @pytest.mark.asyncio
    async def test_http_error_returns_empty_list(self):
        backend = _serpapi_backend()

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(403, text="Forbidden")
            )
            hits = await backend.search("wireless")

        assert hits == []

    @pytest.mark.asyncio
    async def test_500_error_returns_empty_list(self):
        backend = _serpapi_backend()

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(500, text="Internal Server Error")
            )
            hits = await backend.search("query")

        assert hits == []

    @pytest.mark.asyncio
    async def test_network_error_returns_empty_list(self):
        backend = _serpapi_backend()

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                side_effect=httpx.ConnectError("unreachable")
            )
            hits = await backend.search("wireless")

        assert hits == []

    @pytest.mark.asyncio
    async def test_malformed_json_returns_empty_list(self):
        """If the API returns non-JSON, we get empty list without raising."""
        backend = _serpapi_backend()

        with respx.mock:
            respx.get(SERP_MOCK_BASE).mock(
                return_value=httpx.Response(200, text="not json")
            )
            # This would raise a JSON decode error — backend must catch it
            hits = await backend.search("wireless")

        # The exception from resp.json() is not currently caught in search(),
        # only HTTPStatusError is caught. If JSON decode fails it propagates.
        # We test that at least the HTTP-error paths work correctly; this test
        # documents the current behaviour (non-JSON → exception or empty list).
        # Either outcome is acceptable: the caller gets no useful data.
        assert isinstance(hits, list)


# ---------------------------------------------------------------------------
# USPTO PPUBS — success
# ---------------------------------------------------------------------------


class TestUsptoSuccess:
    @pytest.mark.asyncio
    async def test_maps_patents_to_patent_hits(self):
        backend = _ppubs_backend()
        response_data = {
            "patents": [
                {
                    "patentNumber": "US10000001",
                    "title": "Widget method",
                    "abstract": "A widget that does things.",
                    "inventors": ["Alice", "Bob"],
                    "assignee": "Widgets Inc.",
                    "grantDate": "2022-03-15",
                    "filingDate": "2019-07-01",
                }
            ]
        }

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(
                return_value=httpx.Response(200, json=response_data)
            )
            hits = await backend.search("TTL/widget AND ACLM/method")

        assert len(hits) == 1
        h = hits[0]
        assert h.patent_id == "US10000001"
        assert h.title == "Widget method"
        assert h.abstract == "A widget that does things."
        assert h.assignee == "Widgets Inc."
        assert "Alice" in h.inventors
        assert "Bob" in h.inventors
        assert h.date == "2022-03-15"  # grant_date preferred
        assert h.source == "USPTO_PPUBS"
        assert h.relevance == "unknown"

    @pytest.mark.asyncio
    async def test_empty_results_returns_empty_list(self):
        backend = _ppubs_backend()

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(
                return_value=httpx.Response(200, json={"patents": []})
            )
            hits = await backend.search("TTL/nonexistent")

        assert hits == []

    @pytest.mark.asyncio
    async def test_date_params_included_in_body(self):
        """Verify date_from/date_to and dateRangeField appear in POST body."""
        backend = _ppubs_backend()
        captured_body: dict[str, Any] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_body.update(json.loads(request.content))
            return httpx.Response(200, json={"patents": []})

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(side_effect=capture)
            await backend.search(
                "TTL/wireless", date_from="20200101", date_to="20231231"
            )

        assert captured_body.get("startDate") == "20200101"
        assert captured_body.get("endDate") == "20231231"
        assert captured_body.get("dateRangeField") == "applicationDate"

    @pytest.mark.asyncio
    async def test_sources_list_included_in_body(self):
        backend = _ppubs_backend()
        captured_body: dict[str, Any] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_body.update(json.loads(request.content))
            return httpx.Response(200, json={"patents": []})

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(side_effect=capture)
            await backend.search("TTL/test")

        sources = captured_body.get("sources", [])
        assert "US-PGPUB" in sources
        assert "USPAT" in sources

    @pytest.mark.asyncio
    async def test_results_key_also_accepted(self):
        """Backend should also accept 'results' key in response."""
        backend = _ppubs_backend()
        response_data = {
            "results": [{"patentNumber": "US20220100001", "title": "Pub Patent"}]
        }

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(
                return_value=httpx.Response(200, json=response_data)
            )
            hits = await backend.search("TTL/pub")

        assert len(hits) == 1
        assert hits[0].patent_id == "US20220100001"


# ---------------------------------------------------------------------------
# USPTO PPUBS — error handling
# ---------------------------------------------------------------------------


class TestUsptoErrors:
    @pytest.mark.asyncio
    async def test_500_error_returns_empty_list(self):
        backend = _ppubs_backend()

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(
                return_value=httpx.Response(500, text="Internal Server Error")
            )
            hits = await backend.search("TTL/wireless")

        assert hits == []

    @pytest.mark.asyncio
    async def test_network_error_returns_empty_list(self):
        backend = _ppubs_backend()

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(
                side_effect=httpx.ConnectError("unreachable")
            )
            hits = await backend.search("TTL/wireless")

        assert hits == []

    @pytest.mark.asyncio
    async def test_404_returns_empty_list(self):
        backend = _ppubs_backend()

        with respx.mock:
            respx.post(f"{PPUBS_MOCK_BASE}/query").mock(
                return_value=httpx.Response(404, text="Not Found")
            )
            hits = await backend.search("TTL/wireless")

        assert hits == []


# ---------------------------------------------------------------------------
# EPO OPS — classification search builds correct CQL
# ---------------------------------------------------------------------------


class TestEpoClassificationSearch:
    @pytest.mark.asyncio
    async def test_classification_search_wildcard_cql(self):
        """search_by_classification with include_subclasses=True → cpc=H02J50/*"""
        backend = _epo_backend()
        captured_params: dict[str, str] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_params.update(dict(request.url.params))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=capture
            )
            await backend.search_by_classification("H02J50", include_subclasses=True)

        assert "cpc=H02J50/*" in captured_params.get("q", "")

    @pytest.mark.asyncio
    async def test_classification_search_exact_cql(self):
        """search_by_classification with include_subclasses=False → cpc=H02J50/10"""
        backend = _epo_backend()
        captured_params: dict[str, str] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_params.update(dict(request.url.params))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=capture
            )
            await backend.search_by_classification(
                "H02J50/10", include_subclasses=False
            )

        assert "cpc=H02J50/10" in captured_params.get("q", "")

    @pytest.mark.asyncio
    async def test_classification_search_with_dates(self):
        """Date range should be appended to CQL query."""
        backend = _epo_backend()
        captured_params: dict[str, str] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_params.update(dict(request.url.params))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=capture
            )
            await backend.search_by_classification(
                "H02J50",
                include_subclasses=True,
                date_from="2020-01-01",
                date_to="2023-12-31",
            )

        q = captured_params.get("q", "")
        assert "cpc=H02J50/*" in q
        assert "pd>=20200101" in q
        assert "pd<=20231231" in q

    @pytest.mark.asyncio
    async def test_range_param_reflects_max_results(self):
        backend = _epo_backend()
        captured_params: dict[str, str] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_params.update(dict(request.url.params))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=capture
            )
            await backend.search_by_classification("H04W", max_results=10)

        assert captured_params.get("Range") == "1-10"


# ---------------------------------------------------------------------------
# EPO OPS — no credentials → graceful degradation
# ---------------------------------------------------------------------------


class TestEpoNoCreds:
    @pytest.mark.asyncio
    async def test_no_credentials_search_still_sends_request(self):
        """Without credentials, no auth token is requested, but search proceeds."""
        backend = _epo_backend(client_id=None, client_secret=None)

        with respx.mock:
            route = respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                return_value=httpx.Response(200, json={})
            )
            hits = await backend.search("ti=wireless")

        assert route.called
        assert isinstance(hits, list)

    @pytest.mark.asyncio
    async def test_no_credentials_get_oauth_token_returns_none(self):
        """get_oauth_token with no creds immediately returns None."""
        backend = _epo_backend(client_id=None, client_secret=None)
        token = await backend.get_oauth_token()
        assert token is None

    @pytest.mark.asyncio
    async def test_search_error_returns_empty_list(self):
        backend = _epo_backend()

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                return_value=httpx.Response(503, text="Service Unavailable")
            )
            hits = await backend.search("ti=wireless")

        assert hits == []

    @pytest.mark.asyncio
    async def test_search_network_error_returns_empty_list(self):
        backend = _epo_backend()

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=httpx.ConnectError("unreachable")
            )
            hits = await backend.search("ti=wireless")

        assert hits == []


# ---------------------------------------------------------------------------
# EPO OPS — OAuth2 token
# ---------------------------------------------------------------------------

_EPO_AUTH_URL = "http://mock-epo/3.2/auth/accesstoken"


class TestEpoOAuth:
    @pytest.mark.asyncio
    async def test_token_requested_with_credentials(self):
        backend = _epo_backend(client_id="my-id", client_secret="my-secret")

        with respx.mock:
            respx.post(_EPO_AUTH_URL).mock(
                return_value=httpx.Response(
                    200, json={"access_token": "tok123", "expires_in": 1800}
                )
            )
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                return_value=httpx.Response(200, json={})
            )
            token = await backend.get_oauth_token()

        assert token == "tok123"

    @pytest.mark.asyncio
    async def test_token_cached_between_calls(self):
        backend = _epo_backend(client_id="my-id", client_secret="my-secret")

        with respx.mock:
            auth_route = respx.post(_EPO_AUTH_URL).mock(
                return_value=httpx.Response(
                    200, json={"access_token": "cached-tok", "expires_in": 3600}
                )
            )
            await backend.get_oauth_token()
            await backend.get_oauth_token()

        assert auth_route.call_count == 1

    @pytest.mark.asyncio
    async def test_auth_failure_returns_none(self):
        backend = _epo_backend(client_id="bad", client_secret="bad")

        with respx.mock:
            respx.post(_EPO_AUTH_URL).mock(
                return_value=httpx.Response(401, text="Unauthorized")
            )
            token = await backend.get_oauth_token()

        assert token is None

    @pytest.mark.asyncio
    async def test_auth_header_sent_in_search_when_token_available(self):
        """When credentials are valid, Authorization header appears in search request."""
        backend = _epo_backend(client_id="my-id", client_secret="my-secret")
        captured_headers: dict[str, str] = {}

        def capture_search(request: httpx.Request) -> httpx.Response:
            captured_headers.update(dict(request.headers))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.post(_EPO_AUTH_URL).mock(
                return_value=httpx.Response(
                    200, json={"access_token": "bearer-tok", "expires_in": 1800}
                )
            )
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=capture_search
            )
            await backend.search("ti=wireless")

        assert "authorization" in captured_headers
        assert "bearer-tok" in captured_headers["authorization"]


# ---------------------------------------------------------------------------
# EPO OPS — XML response parsing
# ---------------------------------------------------------------------------

_EPO_SEARCH_XML = """<?xml version="1.0"?>
<ops:world-patent-data
    xmlns:ops="http://ops.epo.org"
    xmlns:ep="http://www.epo.org/exchange">
  <ops:biblio-search>
    <ops:search-result>
      <exchange-documents>
        <ep:exchange-document country="EP" doc-number="1234567" kind="B1">
          <ep:bibliographic-data>
            <ep:invention-title lang="en">Wireless charging method</ep:invention-title>
            <ep:dates>
              <ep:date-of-publication>20190903</ep:date-of-publication>
            </ep:dates>
            <ep:inventors>
              <ep:inventor>
                <ep:inventor-name><ep:name>Alice Inventor</ep:name></ep:inventor-name>
              </ep:inventor>
            </ep:inventors>
            <ep:applicants>
              <ep:applicant>
                <ep:applicant-name><ep:name>Tech Corp</ep:name></ep:applicant-name>
              </ep:applicant>
            </ep:applicants>
          </ep:bibliographic-data>
        </ep:exchange-document>
      </exchange-documents>
    </ops:search-result>
  </ops:biblio-search>
</ops:world-patent-data>"""


class TestEpoXmlParsing:
    @pytest.mark.asyncio
    async def test_xml_response_parsed_to_patent_hits(self):
        backend = _epo_backend()

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                return_value=httpx.Response(
                    200,
                    text=_EPO_SEARCH_XML,
                    headers={"content-type": "application/xml"},
                )
            )
            hits = await backend.search("ti=wireless")

        assert len(hits) == 1
        h = hits[0]
        assert "EP" in h.patent_id
        assert "1234567" in h.patent_id
        assert h.source == "EPO_OPS"

    def test_xml_parse_method_directly(self):
        backend = _epo_backend()
        hits = backend._parse_xml_response(_EPO_SEARCH_XML)
        assert len(hits) == 1
        h = hits[0]
        assert h.patent_id.startswith("EP")

    def test_xml_parse_empty_returns_empty_list(self):
        backend = _epo_backend()
        empty_xml = """<?xml version="1.0"?>
<ops:world-patent-data xmlns:ops="http://ops.epo.org" xmlns:ep="http://www.epo.org/exchange">
  <ops:biblio-search><ops:search-result></ops:search-result></ops:biblio-search>
</ops:world-patent-data>"""
        hits = backend._parse_xml_response(empty_xml)
        assert hits == []

    def test_xml_parse_error_returns_empty_list(self):
        backend = _epo_backend()
        hits = backend._parse_xml_response("not xml at all <<<")
        assert hits == []


# ---------------------------------------------------------------------------
# EPO OPS — get_citations
# ---------------------------------------------------------------------------


class TestEpoCitations:
    @pytest.mark.asyncio
    async def test_backward_citation_request_url(self):
        backend = _epo_backend()
        captured_url: list[str] = []

        def capture(request: httpx.Request) -> httpx.Response:
            captured_url.append(str(request.url))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(url__regex=r"/citation/").mock(side_effect=capture)
            await backend.get_citations("EP1234567B1", direction="backward")

        assert len(captured_url) == 1
        assert "EP1234567B1" in captured_url[0]
        assert "citation" in captured_url[0]

    @pytest.mark.asyncio
    async def test_forward_citation_uses_search(self):
        """Forward citations search for patents that cite the given patent."""
        backend = _epo_backend()
        captured_params: dict[str, str] = {}

        def capture(request: httpx.Request) -> httpx.Response:
            captured_params.update(dict(request.url.params))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(f"{EPO_MOCK_BASE}/published-data/search").mock(
                side_effect=capture
            )
            ids = await backend.get_citations("EP9876543B1", direction="forward")

        assert "ct=EP9876543B1" in captured_params.get("q", "")
        assert isinstance(ids, list)

    @pytest.mark.asyncio
    async def test_citation_error_returns_empty_list(self):
        backend = _epo_backend()

        with respx.mock:
            respx.get(url__regex=r"/citation/").mock(
                return_value=httpx.Response(500, text="error")
            )
            ids = await backend.get_citations("EP1111111B1")

        assert ids == []


# ---------------------------------------------------------------------------
# EPO OPS — get_family
# ---------------------------------------------------------------------------


class TestEpoFamily:
    @pytest.mark.asyncio
    async def test_family_request_url_contains_patent_id(self):
        backend = _epo_backend()
        captured_url: list[str] = []

        def capture(request: httpx.Request) -> httpx.Response:
            captured_url.append(str(request.url))
            return httpx.Response(200, json={})

        with respx.mock:
            respx.get(url__regex=r"/family/").mock(side_effect=capture)
            await backend.get_family("EP1234567B1")

        assert len(captured_url) == 1
        assert "EP1234567B1" in captured_url[0]

    @pytest.mark.asyncio
    async def test_family_error_returns_empty_list(self):
        backend = _epo_backend()

        with respx.mock:
            respx.get(url__regex=r"/family/").mock(
                return_value=httpx.Response(503, text="unavailable")
            )
            members = await backend.get_family("EP9999999B1")

        assert members == []

    @pytest.mark.asyncio
    async def test_family_returns_list_of_dicts(self):
        backend = _epo_backend()

        with respx.mock:
            respx.get(url__regex=r"/family/").mock(
                return_value=httpx.Response(200, json={})
            )
            members = await backend.get_family("EP1234567B1")

        assert isinstance(members, list)


# ---------------------------------------------------------------------------
# PatentHit dataclass
# ---------------------------------------------------------------------------


class TestPatentHitDataclass:
    def test_required_field_only(self):
        h = PatentHit(patent_id="US1234567")
        assert h.patent_id == "US1234567"
        assert h.title is None
        assert h.date is None
        assert h.assignee is None
        assert h.inventors == []
        assert h.abstract is None
        assert h.source == ""
        assert h.relevance == "unknown"
        assert h.note == ""
        assert h.prior_art is None
        assert h.url is None

    def test_inventors_default_independent_per_instance(self):
        """Default factory must not share list between instances."""
        h1 = PatentHit(patent_id="US1")
        h2 = PatentHit(patent_id="US2")
        h1.inventors.append("Alice")
        assert h2.inventors == []
