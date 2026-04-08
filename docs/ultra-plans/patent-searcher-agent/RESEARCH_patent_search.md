# Patent Search Research: Deep Patent Search AI Agent

Research compiled: 2026-04-07

---

## Table of Contents

1. [Advanced Patent Search Query Syntax](#1-advanced-patent-search-query-syntax)
2. [IPC/CPC Classification System](#2-ipccpc-classification-system)
3. [Prior Art Search Strategies](#3-prior-art-search-strategies)
4. [Niche and Obscure Patent Finding Techniques](#4-niche-and-obscure-patent-finding-techniques)
5. [APIs for Programmatic Patent Search](#5-apis-for-programmatic-patent-search)
6. [LLM/AI Patent Search Best Practices](#6-llmai-patent-search-best-practices)
7. [Source URLs](#7-source-urls)

---

## 1. Advanced Patent Search Query Syntax

### 1.1 USPTO Patent Public Search

**URL:** https://ppubs.uspto.gov/

**Boolean Operators:**
- `AND`, `OR`, `NOT`, `XOR` (XOR is unique to USPTO)
- `NOT AND`, `NOT OR` variants supported

**Proximity Operators (order of execution matters):**

| Operator | Behavior |
|---|---|
| `ADJ` | Second term immediately follows first, same sentence, in order |
| `ADJn` | Second term within n words of first, in order (e.g., `ADJ4`) |
| `NEAR` | Both terms within n words, any order, same sentence |
| `NEARn` | Both terms within n words, any order |
| `WITH` | Both terms in the same sentence |
| `SAME` | Both terms in the same paragraph |
| `NOT ADJ` | Second appears before first, or not in same sentence |
| `NOT NEAR` | Not in same sentence, any order |
| `NOT WITH` | Not in same sentence |
| `NOT SAME` | Not in same paragraph |

**Important:** USPTO does NOT count stopwords when calculating proximity distances. In "analog and a digital computer," "and" and "a" are stopwords, so `analog ADJ1 digital` would match.

**Operator precedence (lowest to highest priority):**
1. Parentheses (executed first)
2. ADJ and NEAR (left to right)
3. WITH
4. SAME
5. AND and NOT (left to right)
6. XOR
7. OR (executed last)

**Field Codes (Searchable Indexes / Aliases):**

USPTO uses `.FIELDCODE.` notation (dot-wrapped). For CPC, omit spaces: `A45B19/04.cpc.`

| Field | Field Code | Notes |
|---|---|---|
| Title | `.TI.` or `.TTL.` | |
| Abstract | `.AB.` | |
| Claims | `.CLM.` | |
| Detailed Description | `.DETX.` | |
| Brief Summary | `.BSUM.` | |
| Inventor | `.IN.` or `.INV.` | |
| Inventor Group | `.INGP.` | |
| Applicant Name | `.AANM.` | |
| Applicant Data | `.AAD.` | Composite |
| Assignee Name | `.ASNM.` | |
| Attorney/Agent | `.ATT.` or `.ATTY.` | |
| Patent Number | `.PN.` | |
| Application Serial Number | `.AP.` or `.APN.` | |
| Application Filing Date | `.AD.` or `.AFD.` or `.APD.` | |
| Application Filing Year | `.AY.` | |
| Patent Issue Date | `.PD.` | |
| Patent Issue Year | `.PY.` | |
| CPC Classification | `.CPC.` | No spaces: `A45B19/04.cpc.` |
| IPC Classification | `.CIPC.` or `.IPC.` | |
| Current US Classification | `.CCLS.` | |
| Document Kind Code | `.KD.` | |
| PCT Number | `.PCT.` | |
| Document ID | `.DID.` | |

**Wildcards/Truncation:**
- `*` — truncation (zero or more characters): `comput*` matches computer, computing, computation
- `?` — single character wildcard: `wom?n` matches woman, women

**Search History (L-Sets):**
Each query saves as L1, L2, L3... and can be combined: `L1 AND L3 NOT L2`

**Example queries:**
```
(lidar OR radar).TI. ADJ4 sensor.AB.
A45B19/04.cpc.
"autonomous vehicle".AB. AND G08G1/16.cpc.
airbag$ AND safety.TI.
```

**Official reference:** https://www.uspto.gov/patents/search/patent-public-search/searchable-indexes

---

### 1.2 Espacenet (EPO)

**URL:** https://worldwide.espacenet.com/

Espacenet uses **CQL (Contextual Query Language)** in its Smart Search expert mode.

**Boolean Operators:**
- `AND` (default when space used), `OR`, `NOT`
- Default operator is AND (left-to-right precedence)
- NOT can only be used once per query in basic mode

**Field Identifiers (prefix with `fieldcode=`):**

| Field Code | Description | Example |
|---|---|---|
| `ti` | Title | `ti="electric motor"` |
| `ab` | Abstract | `ab="fuel cell"` |
| `ctxt` | Title, abstract, OR claims | `ctxt="solar panel"` |
| `txt` | Title and abstract (default keyword) | `txt=battery` |
| `extftxt` | Full-text (requires full-text DB) | `extftxt="pharmaceutical product"` |
| `cl` | Any classification (IPC or CPC) | `cl=H01M` |
| `cpc` | CPC only | `cpc=H01M10/00` |
| `ipc` | IPC only | `ipc=B60L` |
| `pn` | Publication number | `pn=EP1000000` |
| `ia` | Inventor OR applicant name | `ia="Smith"` |
| `pd` | Publication date | `pd=20080107` or `pd=20200101:20241231` |
| `num` | Any number field | `num=EP1000000` |

**Date range format for `pd`:** `pd=YYYYMMDD:YYYYMMDD`

**Comparison Operators:**
- `=` (default equals)
- `any` (any of the terms between quotes count individually)
- `all` (all terms)
- `within`

**Proximity Operators:**
- `prox/distance<n` — within n words, any order
- `prox/ordered` — in order
- `prox/unit=sentence` — same sentence
- `prox/unit=paragraph` — same paragraph

Example: `ctxt prox/distance<3 (mouse trap)` — "mouse" within 3 words of "trap" in title/abstract/claims

**Classification with `/low` operator:**
- `cpc=H01M/low` — includes all CPC subgroups under H01M
- `ipc=B60L/low` — all IPC subgroups under B60L
- Cannot combine with `any`/`all` operators

**Notes:**
- Do NOT use wildcards with classification codes
- Do NOT put spaces in classification codes: enter `A63B49` not `A63B 49`
- Classification codes are not case-sensitive
- Entering just the group (e.g., `H01M`) auto-retrieves all subgroups
- Max 10 search terms per field
- No diacritical characters in worldwide database

**Example queries:**
```
ti="autonomous driving" AND ia=Tesla
cpc=G06F AND ab="machine learning" AND pd=20200101:20251231
ia=Google AND (ab="autonomous driving" OR ab="autonomous vehicle")
mouse prox/distance<3 trap
```

**Official reference pages:**
- Field identifiers: https://worldwide.espacenet.com/help?topic=fieldidentifier&locale=en_EP&method=handleHelpTopic
- Operators: https://worldwide.espacenet.com/help?topic=operators&locale=en_EP&method=handleHelpTopic
- Expert mode: https://worldwide.espacenet.com/patent/help/smart-search-expert-mode

---

### 1.3 Google Patents

**URL:** https://patents.google.com/

**Boolean Operators:**
- `AND`, `OR`, `NOT` — must be CAPITALIZED
- Default operator is AND with left associativity
- `safety OR seat belt` is parsed as `(safety OR seat) AND belt` — use parentheses to override

**Field Operators:**
| Operator | Scope |
|---|---|
| `TI=(keyword)` | Title |
| `AB=(keyword)` | Abstract |
| `CL=(keyword)` | Claims |
| `CPC=B60R22` | Exact CPC match |
| `CPC=B60R22/low` | CPC + all child classifications |
| `cpc:B60R22/00` | Alternative CPC syntax |
| `inventor:"Name"` | Inventor filter |
| `assignee:"Company"` | Assignee filter |
| `country:US` | Country/jurisdiction filter |
| `status:grant` or `status:GRANT` | Granted only |
| `status:APPLICATION` | Pending applications |
| `language:German` | Language filter |
| `before:"2020"` | Filed before date |
| `after:"2018"` | Filed after date |

**Date range operators (for API use):**
- `before:priority:YYYYMMDD` — before priority date
- `after:filing:YYYYMMDD` — after filing date
- Types: `priority`, `filing`, `publication`

**Proximity Operators (affect ranking, not retrieval):**
| Operator | Meaning |
|---|---|
| `NEAR/x` or `NEARx` | Within x words, any order |
| `ADJ/x` or `ADJx` | Within x words, same order |
| `WITH` | Within 20 words, any order |
| `SAME` | Within 200 words, any order |

**Warning:** Wildcards, ADJ, NEAR, WITH, and SAME are NOT robust in Google Patents and do not work consistently.

**Wildcards:**
- `*` truncation: `comput*` matches computer/computing/computation

**Combined example:**
```
assignee:(Merck OR Novartis) AND CL:((metformin OR biguanide) AND ("extended release" OR "sustained release"))
(cpc:C07K16/2878 OR "PD-1 inhibitor") AND (cpc:A61P35/00 OR cancer OR tumor)
```

**Official reference:** https://support.google.com/faqs/answer/7049475?hl=en

---

### 1.4 WIPO PatentScope

**URL:** https://patentscope.wipo.int/

**Field Codes:**
| Field Code | Description |
|---|---|
| `IN:` | Inventor name |
| `PA:` | Applicant/Assignee |
| `EN_AB:` | English Abstract |
| `EN_CL:` | English Claims |
| `EN_TI:` | English Title |
| `DP:` | Publication date |
| `AD:` | Application date |
| `IC:` | IPC classification |
| `FP:` | Filing date |

**Date range syntax:** `DP:[2009 TO 2017]` (Lucene-style range queries)

**Example:**
```
IN:(Thrun) AND DP:[2009 TO 2017] AND EN_AB:(autonomous or driverless)
```

**Special features:**
- **Stemming:** Enabled by default (searches root forms). Untick "Stem" for exact matching.
- **Proximity operators:** Available in Advanced Search
- **Cross-lingual semantic search:** Can search in one language and find patents in others

---

### 1.5 Syntax Comparison Table

| Feature | USPTO PPS | Espacenet | Google Patents | WIPO PatentScope |
|---|---|---|---|---|
| Boolean | AND, OR, NOT, XOR | AND, OR, NOT | AND, OR, NOT | AND, OR, NOT |
| Proximity ordered | `ADJn` | `prox/ordered/distance=n` | `ADJ/n` (unreliable) | `W/n` |
| Proximity unordered | `NEARn` | `prox/distance<n` | `NEAR/n` (unreliable) | `NEAR/n` |
| Same sentence | `WITH` | `prox/unit=sentence` | `WITH` (unreliable) | - |
| Same paragraph | `SAME` | `prox/unit=paragraph` | `SAME` (unreliable) | - |
| Title field | `.TI.` | `ti=` | `TI=` | `EN_TI:` |
| Abstract field | `.AB.` | `ab=` | `AB=` | `EN_AB:` |
| Claims field | `.CLM.` | `ctxt=` (approx.) | `CL=` | `EN_CL:` |
| CPC field | `.CPC.` | `cpc=` | `CPC=` or `cpc:` | `IC=` (IPC only) |
| IPC field | `.CIPC.` | `ipc=` | (via CPC) | `IC=` |
| Inventor | `.INV.` | `ia=` | `inventor:` | `IN:` |
| Assignee | `.ASNM.` | `ia=` (combined) | `assignee:` | `PA:` |
| Date range | `.AD.=YYYYMMDD:YYYYMMDD` | `pd=YYYYMMDD:YYYYMMDD` | `before:/after:` | `DP:[Y TO Y]` |
| Wildcard | `*`, `?` | `*`, `?` | `*` | `*`, `?` |
| Subclass hierarchy | Manual | `cpc=H01M/low` | `CPC=H01M/low` | Manual |
| Search history | L-sets | None | None | None |
| Full-text search | Yes (USOCR for pre-1976) | `extftxt=` (selected docs) | Yes | Yes (PCT docs) |

---

## 2. IPC/CPC Classification System

### 2.1 Overview

**IPC (International Patent Classification):**
- Governed by the Strasbourg Agreement (1971)
- Administered by WIPO
- Used by 100+ countries, 4 regional offices, and WIPO itself
- ~80,145+ codes (as of 2025)
- Updated annually (current: IPC 2025.01)
- Language-independent hierarchical symbol system

**CPC (Cooperative Patent Classification):**
- Joint system by USPTO + EPO (launched January 1, 2013)
- Extension and refinement of IPC — all IPC codes exist in CPC
- ~254,249+ codes (far more granular than IPC)
- Includes **Section Y** for cross-cutting themes (e.g., climate change mitigation)
- The primary system for US and European patent searching

### 2.2 Hierarchical Structure

Both IPC and CPC share this five-tier hierarchy:

```
Section  →  Class  →  Subclass  →  Main Group  →  Subgroup
    H           H04        H04L          9/00             9/32
```

**Breaking down H04L 9/32:**
- `H` = Section (Electricity)
- `H04` = Class (Electric Communication Technique)
- `H04L` = Subclass (Transmission of Digital Information)
- `H04L 9/00` = Main Group (Cryptographic mechanisms)
- `H04L 9/32` = Subgroup (means for verifying identity)

**8 Sections (A through H + Y):**
- A: Human Necessities
- B: Performing Operations; Transporting
- C: Chemistry; Metallurgy
- D: Textiles; Paper
- E: Fixed Constructions
- F: Mechanical Engineering; Lighting; Heating; Weapons
- G: Physics
- H: Electricity
- Y: General Tagging (cross-sectional technologies — CPC only)

### 2.3 Identifying the Right Classes

**Step-by-step workflow:**

1. **Keyword-to-classification:** Search a known patent in the area → note its CPC codes
2. **CPC Browser:** Use https://www.cooperativepatentclassification.org/cpcSchemeAndDefinitions/CPC to browse hierarchy
3. **WIPO IPC Navigator:** https://ipcpub.wipo.int/ for IPC browsing
4. **Patent Classification Explorer:** https://patentclassificationexplorer.com/ — free tool for browsing both IPC and CPC
5. **PQAI:** AI-powered CPC prediction from natural language description
6. **USPTO CPC Definition lookup:** definitions explain the scope of each code
7. **Espacenet Classification Search:** Enter keywords, let it suggest codes

**WIPO AI Tools:**
- **IPCCAT:** Automated IPC classification assistance tool (classify.wipo.int)
- **STATS:** IPC predictions based on statistical analysis

**Hierarchy navigation tip:** A CPC subgroup automatically includes all its child subgroups when searched without qualification. Use `/low` in Espacenet or `CPC=B60R22/low` in Google Patents to explicitly include all children.

### 2.4 Why Use Classification vs. Keywords

| Advantage | Explanation |
|---|---|
| Language independence | Finds patents in any language with the same code |
| Terminology variation | Pre-1976 patents and foreign patents with no English abstract are reachable |
| Technical precision | Groups by function, not vocabulary |
| Adjacent technology discovery | Browsing hierarchy reveals related fields |
| Examiner alignment | Patent examiners search heavily by CPC — using it searches like they do |
| No-text documents | Some patent documents have no title/abstract; classification is the only way to find them |

### 2.5 Key Classification Navigation Resources

- CPC Scheme browser: https://www.cooperativepatentclassification.org/cpcSchemeAndDefinitions/CPC
- WIPO IPC: https://www.wipo.int/en/web/classification-ipc
- WIPO Guide to IPC 2025: https://www.wipo.int/edocs/pubdocs/en/wipo-guide-ipc-2025-en-guide-to-the-international-patent-classification-2025.pdf
- USPTO Classification Portal: https://www.uspto.gov/patents/search/classification-standards-and-development
- Patent Classification Explorer: https://patentclassificationexplorer.com/
- WIPO Analytics Handbook - Classification chapter: https://wipo-analytics.github.io/handbook/classification.html

---

## 3. Prior Art Search Strategies

### 3.1 The Core Workflow: Keyword → Classification → Citation Chaining

**USPTO's recommended multi-step strategy:**

1. **Brainstorm synonyms and alternative terminology**
   - Think of all names for the concept: umbrella = parasol = sunshade
   - Include technical engineering terms alongside common words
   - Document trade names, brand names, historical terms
   - Consider how terminology may have changed over time

2. **Broad keyword search**
   - Start with Patent Public Search (Basic or Advanced)
   - Use truncation (`airbag$`, `comput*`)
   - Use quotes for phrases (`"image viewer"`)
   - Avoid relying only on common words — add technical specificity

3. **Identify CPC/IPC codes from keyword results**
   - From your keyword hits, note all CPC/IPC codes assigned to relevant patents
   - Use the CPC Browser to explore parent/sibling/child codes
   - Construct a classification-only search to find documents missed by keywords

4. **In-depth review**
   - Examine complete patent documents: drawings, specification, claims
   - Claims define scope; specification provides context

5. **Citation chaining (backward + forward)**
   - **Backward citations:** Patents cited by your seed patent = prior art that examiner already considered
   - **Forward citations:** Patents that later cited your seed = related inventions filed after
   - Chain multiple generations: seed → backward → backward's backward (second generation)
   - Use INPADOC family to capture the same patent across jurisdictions

6. **Broaden with foreign and non-patent sources**
   - Espacenet for 140+ million patents from 100+ countries
   - WIPO PatentScope for PCT applications
   - Google Scholar, arXiv, IEEE Xplore, PubMed for NPL

### 3.2 Forward/Backward Citation Analysis in Detail

**Tools with citation support:**
- **Espacenet:** Full INPADOC family + CCD (Common Citation Document) viewer
- **Google Patents:** "Cited by" tab for forward citations; references section for backward
- **Lens.org:** Cross-cites between patents and scholarly literature
- **PatentScope (WIPO):** Common Citation Document access
- **Global Dossier:** IP5 offices (USPTO, EPO, JPO, KIPO, CNIPA) combined citation data

**INPADOC patent family (Espacenet):**
- Broad family definition: any patent sharing a priority OR connected via domestic filing
- Includes all jurisdictions where the same invention was filed
- More expansive than "simple family" (requires identical priority set)
- Best for: prior art analysis, legal status, jurisdiction coverage

**Simple family:**
- All documents sharing exactly the same combination of priorities
- Smaller set; equivalent applications only

**Citation counting tip:** To count unique citing documents across a family, count DISTINCT citing document numbers — do not sum per-family-member counts (will double-count patents that cite multiple family members).

### 3.3 Patent Family Search (Equivalent Patents Across Jurisdictions)

**Why it matters:** An invention may be patented in 20+ countries. Finding the US patent gives you access to the English claims; finding the EP patent gives you EPO examination history.

**How to execute:**
1. Find one patent in any jurisdiction
2. Use Espacenet's "Patent family" tab → INPADOC family shows all equivalents
3. Or use PatentScope WIPO family search
4. Global Dossier (https://globaldossier.uspto.gov/) provides IP5 family + file wrapper access

**Jurisdiction coverage strategy:**
- WO (PCT) = WIPO international application, filed before nationalization
- EP = European Patent Office (covers ~40 European countries with one filing)
- US = USPTO
- CN = China (CNIPA)
- JP = Japan (JPO)
- KR = Korea (KIPO)
- Important: Not every technology is filed in every jurisdiction — check smaller offices for niche tech

### 3.4 Non-Patent Literature (NPL) as Prior Art

**What counts as NPL:**
- Peer-reviewed journal articles
- Conference proceedings and papers
- Dissertations and theses
- Technical standards (IEEE, ISO, IEC, ANSI, DIN, etc.)
- Industry white papers and technical reports
- Product manuals and datasheets
- Grant reports and government-funded research deliverables
- Wikipedia and web pages (timestamped)
- Defensive publications (IP.com, Research Disclosure)

**Key NPL databases:**

| Database | Coverage | Cost |
|---|---|---|
| Google Scholar | Broad academic + preprints | Free |
| arXiv | CS, physics, math, biology preprints | Free |
| PubMed / MEDLINE | Biomedical / life sciences | Free |
| IEEE Xplore | Engineering, electronics, computing | Subscription (some free) |
| Web of Science | Multidisciplinary, citation analysis | Subscription |
| SciFinder (CAS) | Chemistry, materials | Subscription |
| ACM Digital Library | Computer science | Subscription/free partial |
| ScienceDirect | Multi-domain Elsevier journals | Subscription |
| SpringerLink | Multi-domain Springer journals | Subscription |
| JSTOR | Humanities, social science, science | Subscription |
| IP.com | Defensive publications | Subscription |
| Research Disclosure | Defensive publications | Subscription |
| Dissertation Abstracts | Theses worldwide | Subscription |

**USPTO STIC:** Patent examiners access 102,000+ electronic journals and 487,000+ electronic books through the Scientific and Technical Information Center.

**WIPO + NPL:** Since 2021, WIPO has added Open Access publications to PatentScope, indexed with IPC codes.

**Recommended approach:**
1. Check free resources first (Google Scholar, arXiv, PubMed)
2. Note document: author, title, date, DOI/URL, database searched, search string
3. Timestamp all downloaded documents for defensibility

---

## 4. Niche and Obscure Patent Finding Techniques

### 4.1 Handling Unusual or Domain-Specific Terminology

The fundamental problem: if you search only keywords, you are at the mercy of the vocabulary the inventor chose. Solutions:

**Strategy 1: Classification-first approach**
- Find a known patent in the area → note its CPC codes
- Search by classification code alone, ignoring vocabulary
- This retrieves patents regardless of what words the inventor used
- Works across languages: CPC is language-agnostic

**Strategy 2: Multiple synonym expansion**
- Map all known terms for the concept across disciplines
- Example: "airbag" also appears as "air bag," "inflatable restraint," "SRS" (Supplemental Restraint System), "cushion," "inflatable cushion," "passive restraint"
- Historical usage: terminology changes over decades — look at older patents in the class to identify historical terms

**Strategy 3: Component-based search**
- Break invention into sub-components, each with its own terminology
- Search each component separately, then intersect results

**Strategy 4: Semantic/AI search**
- Tools like PQAI, PatSnap, or Semantic Scholar can match concepts not just keywords
- Particularly useful when the technology has no established vocabulary

### 4.2 Historical Patents (Pre-1976 USPTO, Pre-1900 Worldwide)

**The core limitation:** USPTO PatFT only has full-text from 1976 onward. Pre-1976 patents are searchable only by patent number, issue date, and current US classification number.

**Available resources for pre-1976 US patents:**

| Resource | Coverage | Notes |
|---|---|---|
| USPTO PatFT | 1790–present (images); 1976–present (full text) | Pre-1976: classification search only |
| Google Patents | 1790–present | Has OCR'd full text for many pre-1976 patents; best free option |
| PubWEST | 1920–1975+ (OCR full text) | Available at USPTO and Patent & Trademark Depository Libraries only |
| NYPL Print Collection | 1790–1925 | Physical print indexes |

**Print indexes for very old patents:**
- *Subject-Matter Index of Patents* (1790–1873)
- *Annual Report of the Commissioner of Patents* (1846–1925)

**Google Patents advantage for historical:** Has applied OCR to pre-1976 patent images, allowing keyword searches not possible in PatFT. This is the best free tool for historical text search.

**For pre-1900 worldwide patents:**
- Espacenet covers some collections dating to 1836
- British Library provides UK patent access from 1617
- WIPO PatentScope includes US patents from 1790

**Strategy for pre-1976 searches:**
1. Use classification (CPC/USPC) as primary strategy — codes were retroactively applied
2. Use Google Patents for text search (OCR quality varies)
3. Check NYPL or other library resources for physical access to print indexes
4. USPTO PatFT for image viewing after finding patent number

### 4.3 Small Entity and Individual Inventor Patents

**Finding them:**
- Individual inventors rarely appear in assignee searches (many self-assign or assign to LLC)
- Search inventor name: `.INV.` in USPTO, `ia=` in Espacenet
- Small entities often file more basic language — less technical jargon
- Classification search helps because examiners classify by technical content, not vocabulary sophistication

**Provisional applications:**
- Not searchable directly
- Only discoverable after non-provisional is filed and published (18 months from earliest priority)
- Access via USPTO Public PAIR or ODP once published, which lists the priority provisional
- Caution: provisionals are often not published and may expire without filing non-provisional

**Searching individual inventor patterns:**
- No assignee in many cases → leave assignee field blank
- Use inventor name + classification + date range
- Check Google Patents "Assignee" field for personal names vs. company names

### 4.4 International Filings (PCT/WO)

**PCT application lifecycle and search implications:**
1. **Priority filing** (national): Day 0 — not yet publicly searchable
2. **PCT filing** (WO): Up to 12 months after priority — international phase begins
3. **PCT Publication**: ~18 months from priority date — first point of public searchability
4. **ISR publication**: International Search Report released, with prior art citations
5. **National phase entry**: 30 months from priority — becomes national/regional patents

**Searching PCT/WO applications:**
- WIPO PatentScope: Primary database for WO applications, includes full text
- Espacenet: Also indexes WO applications with INPADOC family links
- Google Patents: Indexes WO numbers (e.g., `WO2019123456A1`)

**Key insight:** A WO application may be the only public disclosure of an invention for its first 30 months. Search WO before the inventor nationalizes — especially important for FTO and competitive intelligence.

**Country code reference for international search:**
| Code | Office |
|---|---|
| WO | WIPO (PCT application) |
| EP | European Patent Office |
| US | USPTO |
| CN | China CNIPA |
| JP | Japan JPO |
| KR | Korea KIPO |
| DE | Germany |
| GB | United Kingdom |
| FR | France |
| AU | Australia |
| CA | Canada |
| IN | India |
| BR | Brazil |

**INPADOC family tip:** One WO application may spawn 10–40 national patents. Find the WO, then use INPADOC to see all national entries.

---

## 5. APIs for Programmatic Patent Search

### 5.1 USPTO Patent Public Search / Open Data Portal (Free)

**Base URL:** https://data.uspto.gov/

**Authentication:** Free API key from developer.uspto.gov (required for batch use; optional for low-volume)

**Key APIs:**

| API | Description | URL |
|---|---|---|
| Patent File Wrapper Search | Search published applications and patents by metadata | `data.uspto.gov/apis/patent-file-wrapper/search` |
| Patent File Wrapper Documents | Retrieve file wrapper documents | `data.uspto.gov/apis/patent-file-wrapper/documents` |
| Patent Assignment Search | Patent assignment/ownership records | API via developer.uspto.gov |
| PTAB API v3 | Patent Trial and Appeal Board decisions | `data.uspto.gov` (Swagger available) |
| Office Action APIs | Office actions, citations, rejections | Migrating to ODP |

**Limitations:**
- Primarily bibliographic + file wrapper data
- No full-text claim/description search via these endpoints
- PatFT/AppFT classic interfaces still available for Boolean text search

**PatentsView PatentSearch API (Free):**
- **URL:** https://patentsview.org/
- **Key:** Registration required for API key; sent as `X-Api-Key` header
- **Engine:** ElasticSearch-based; 7 unique endpoints
- **Data:** US granted patents 1976–present, updated quarterly; CPC retroactively applied to all patents
- **Rate limit:** 45 requests/minute
- **Note:** Legacy PatentsView API discontinued May 1, 2025 (returns 410 Gone); use new PatentSearch API
- **Migration:** Moving to data.uspto.gov on March 20, 2026
- **Example use:** "Which companies hold patents in 3D printing?" style queries

**Docs:** https://patentsview.org/apis/purpose

---

### 5.2 EPO OPS (Open Patent Services) (Free up to limits)

**Base URL:** https://ops.epo.org/

**Authentication:** OAuth 2.0. Register at https://developers.epo.org/ to get Consumer Key + Secret.

**Free tier:** 4 GB of data per week (not per month as sometimes stated in older docs)

**Data format:** XML (requires parsing; no native JSON)

**Key endpoints:**

```
GET /published-data/search?q=ti=electric AND pa=tesla&Range=1-25
GET /published-data/search/biblio?q=ti=battery AND ic=H01M
GET /published-data/{type}/{format}/{number}/biblio
GET /published-data/{type}/{format}/{number}/abstract
GET /published-data/{type}/{format}/{number}/fulltext
GET /published-data/{type}/{format}/{number}/description
GET /published-data/{type}/{format}/{number}/claims
GET /published-data/{type}/{format}/{number}/equivalents
GET /published-data/{type}/{format}/{number}/images
```

**Query fields (CQL-style):**
- `ti=` — title
- `pa=` — applicant/assignee
- `in=` — inventor
- `ic=` or `ipc=` — IPC classification
- `cpc=` — CPC classification
- Country/date filters supported

**Critical limitation:** OPS does NOT support full-text search across descriptions and claims. It is best for bibliographic retrieval, family data, legal status, and images.

**Country/office coverage:** 49 offices including EP, WO, US, GB, DE, FR, JP, CN, KR

**Python client:** `python-epo-ops-client` on PyPI (v4.2.1 as of Sep 2025)

**Error handling:** HTTP 403 = rate limit hit; implement exponential backoff

**Resources:**
- OPS Portal: https://ops.epo.org/
- Developer registration: https://developers.epo.org/
- Python client: https://pypi.org/project/python-epo-ops-client/
- SDK reference: https://github.com/abdullahatrash/epo-ops-sdk/blob/main/OPS.md

---

### 5.3 Espacenet (EPO) — Web Interface Only

No public REST API for Espacenet's full search interface. Use EPO OPS for programmatic access. Espacenet is a browser-based tool.

---

### 5.4 Google Patents — via SerpAPI

Google Patents has no official public API. Access is via third-party wrappers.

**SerpAPI Google Patents endpoint:**
- **URL:** https://serpapi.com/google-patents-api
- **Auth:** API key (paid service; free trial available)
- **Format:** JSON
- **Engine parameter:** `engine=google_patents`

**Key search parameters:**

| Parameter | Description |
|---|---|
| `q` | Query string; supports Boolean `(Coffee) OR (Tea)` |
| `page` | Page number (starts at 1) |
| `num` | Results per page (10–100 max) |
| `sort` | `new` or `old` (by filing/publication date) |
| `before` | Max date: `type:YYYYMMDD` where type = `priority`, `filing`, or `publication` |
| `after` | Min date: same format |
| `inventor` | Comma-separated inventor names |
| `assignee` | Comma-separated assignee names |
| `country` | Comma-separated country codes (e.g., `WO,US`) |
| `language` | ENGLISH, GERMAN, CHINESE, FRENCH, etc. |
| `status` | `GRANT` or `APPLICATION` |
| `type` | `PATENT` or `DESIGN` |
| `litigation` | `YES` or `NO` |
| `clustered` | `true` for classification-based grouping |

**Details endpoint:** `engine=google_patents_details` — returns claims, description, family members, citations, metadata

**MCP server option:** SoftwareStartups/google-patents-mcp (GitHub) wraps SerpAPI as an MCP tool; uses `search_patents` tool with same parameter set.

**Alternative:** searchapi.io also provides a Google Patents API endpoint.

**Docs:**
- https://serpapi.com/google-patents-api
- https://serpapi.com/google-patents-details-api
- https://github.com/SoftwareStartups/google-patents-mcp

---

### 5.5 Lens.org API (Free for non-commercial/academic)

**Base URL:** https://pmr-beta.api.lens.org/

**Auth:** Bearer token (request via Lens user profile; manual approval process)

**Coverage:** 140+ million patent records across global jurisdictions; 120+ searchable fields

**Key endpoints:**

| Method | Endpoint | Description |
|---|---|---|
| POST | `/patent/search` | Full search with JSON body |
| GET | `/patent/{lens_id}` | Retrieve individual patent |
| GET | `/patent/search?query=...&token=TOKEN` | Search via GET params |

**Core request parameters:**
```json
{
  "query": { ... },   // required: valid JSON search
  "sort": [ ... ],    // field-based sorting
  "include": [ ... ], // projection: fields to include
  "exclude": [ ... ], // projection: fields to exclude
  "size": 10,         // page size
  "from": 0,          // offset
  "scroll_id": "...", // cursor pagination
  "scroll": "1m"      // scroll context TTL
}
```

**Searchable fields include:**
- `lens_id`, `publication_number`, `doc_number`, `country`, `kind`, `date_published`
- `title`, `abstract`, `claims`, `description`
- `date_filed`, `filing_country`, `doc_number` (application)
- Applicant: `name`, `address`, `residence_country`
- Inventor: `name`
- Classifications: CPC, IPC, IPCR, national systems
- Citations (patent and non-patent)
- Legal status: `grant_date`, `expiry_date`, `patent_status`, `prosecution_stage`
- Boolean filters: `has_abstract`, `has_claim`, `has_description`, `has_title`

**Access plans:**
- Trial/academic: Free for non-commercial use (apply via profile)
- Commercial: Custom pricing (contact Lens)
- Bulk data downloads available separately

**Swagger UI:** https://api.lens.org/swagger-ui.html
**Docs:** https://docs.api.lens.org/
**GitHub:** https://github.com/cambialens/lens-api-doc

---

### 5.6 WIPO PatentScope API (Paid)

**Protocol:** SOAP/Java-based API (not REST)
**Cost:** 2,000 CHF per calendar year (~$2,200 USD)
**Rate limit:** <10 retrieval actions per minute per subscriber IP

**Free alternative:** PatentScope web interface allows downloading 1,000 or 10,000 records per session when logged in (free account).

**Bulk data:** Bibliographic data + title/abstract XML for current-year applications available for 400 CHF/year.

**API Catalog:** https://apicatalog.wipo.int/
**Contact:** patentscope@wipo.int

---

### 5.7 PatentsView — Free (USPTO)

- **URL:** https://patentsview.org/apis/purpose
- See Section 5.1 for full details
- Best for US patent metadata, inventor disambiguation, entity analysis

---

### 5.8 PQAI API — Semantic Patent Search (Free + Paid)

**URL:** https://projectpq.ai/

**Capabilities:**
- Semantic prior art search from plain English (no Boolean required)
- CPC classification prediction from text
- Concept extraction from invention descriptions
- Patent drawing retrieval

**Pricing:**
- Free tier: 1,000 requests/hour
- $20/month tier: 1,500 requests/month
- Enterprise: $700/month (SLA, custom endpoints, on-premise option)

**Best for:** Natural language → patent search conversion; AI agent integration

---

### 5.9 Commercial / Enterprise Platforms (Paid, No Public Pricing)

**PatSnap:**
- AI-native innovation intelligence platform
- Links 2+ billion structured data points (patents, journals, litigation)
- API access available; also supports MCP integration
- Trusted by 15,000+ organizations
- **Pricing:** Custom enterprise contract — contact sales@patsnap.com
- URL: https://www.patsnap.com/

**Derwent Innovation (Clarivate):**
- Best for legal due diligence, FTO, and validity analysis
- DWPI (Derwent World Patents Index) with 300+ normalized searchable fields
- Curated, enriched patent data with value-added indexing
- **Pricing:** Custom enterprise — contact Clarivate directly
- URL: https://clarivate.com/derwent/

**Orbit Intelligence (Questel):**
- Combines patents + NPL in one platform
- Strong in analytics and visualization
- **Pricing:** Custom enterprise

**Comparison summary:**

| Platform | Best For | Full-Text | API | Cost |
|---|---|---|---|---|
| USPTO ODP | US filing/wrapper data | Limited | REST | Free |
| PatentsView | US metadata & analytics | No | REST | Free |
| EPO OPS | EU/global biblio, family, legal status | No | REST | Free (4GB/wk) |
| Lens.org | Global open research | Yes | REST | Free/Academic |
| Google Patents (SerpAPI) | Quick full-text global search | Yes | Via SerpAPI | SerpAPI pricing |
| PQAI | Semantic/AI prior art | Yes | REST | Free + paid tiers |
| WIPO PatentScope | PCT/WO applications | Yes | SOAP | Free (web) / 2000 CHF (API) |
| PatSnap | AI innovation intelligence | Yes | REST + MCP | Enterprise |
| Derwent Innovation | Legal/FTO/validity | Yes | Yes | Enterprise |

---

## 6. LLM/AI Patent Search Best Practices

### 6.1 Natural Language → Patent Query Conversion

**The fundamental challenge:** Patent drafters intentionally use different terminology from common usage. Patentees use their own lexicon, abstract/generic terms, and unusual paraphrasing to maximize protective scope. LLMs must bridge this vocabulary gap.

**Step-by-step NL → query pipeline:**

1. **Extract core inventive concepts** from the natural language description
   - Use an LLM to identify the 3–7 primary technical elements
   - Separate "what it is" from "what it does" from "how it works"

2. **Generate synonym clusters** for each concept
   - Direct synonyms (airbag → inflatable cushion → passive restraint)
   - Hypernyms (broader terms: "sensor" → "detection device")
   - Hyponyms (narrower terms: "lidar" → "time-of-flight lidar")
   - Domain-specific terms (include both informal and formal technical vocabulary)

3. **Predict CPC/IPC codes**
   - Use PQAI's CPC prediction or an LLM trained on patent data
   - Validate predicted codes against known prior art
   - Expand to parent codes for broader coverage

4. **Generate multiple query variants**
   - Keyword-only query (broadest)
   - Classification-only query
   - Keyword + classification combination
   - Field-specific queries (title-only, claims-only)

5. **Translate to database-specific syntax**
   - Different operators per database (see Section 1.5 comparison table)
   - LLMs can be prompted with database syntax rules to generate valid queries

**Prompt engineering pattern for query generation:**
```
Given this invention description: [DESCRIPTION]
Generate a USPTO Patent Public Search query using:
- Boolean operators: AND, OR, NOT
- Proximity: ADJn for ordered, NEARn for unordered
- Field codes: .TI. .AB. .CLM. .CPC.
- Truncation: * for word endings
Include 3 query variants: broad, medium, and narrow.
```

### 6.2 Concept Expansion Techniques

**Ontological expansion:**
- Synonyms and spelling variants
- Hyponyms (specific types of the concept)
- Hypernyms (broader categories)
- Meronyms (part-of relationships)
- Metonyms

**Technical taxonomy expansion:**
- Find the CPC code for the concept
- Expand to sibling codes (same parent level) for adjacent technologies
- Expand to parent codes for broader technology domain
- Expand to Y-section codes for cross-cutting themes

**Historical terminology expansion:**
- Search the earliest patents in a class to find older vocabulary
- Technology naming often evolves: "wireless telegraphy" → "radio" → "RF communication"
- Check Wikipedia technology history articles for vocabulary lineage

**AI-powered expansion tools:**
- PQAI Concept Extractor: Breaks invention into core elements automatically
- PatSnap AI: Suggests related terms from patent corpus statistics
- Word embeddings trained on patent corpora (academic tools)

**Semantic/embedding-based search:**
- Use vector embeddings of patent claims/abstracts
- Retrieve by cosine similarity to query embedding
- Platforms: PQAI, PatSnap, Patlytics, DeepIP
- Bypasses vocabulary gap entirely — finds "portable electronic apparatus with tactile response system" when searching "mobile device with haptic feedback"

### 6.3 Avoiding Missing Relevant Prior Art

**Common failure modes:**

| Failure Mode | Mitigation |
|---|---|
| Vocabulary gap | Classification search + semantic search |
| Language barrier | Search WO/JP/CN/KR in original or via machine translation |
| Temporal coverage | Explicitly check pre-1976 (Google Patents OCR) + modern databases |
| Database gaps | Use ≥3 databases; no single database is complete |
| NPL gap | Systematic NPL search (Scholar, arXiv, IEEE, PubMed) |
| Citation graph holes | Multi-generation citation chaining (≥2 generations forward + backward) |
| Jurisdiction gaps | Search WO + EP + US + CN + JP minimum for global coverage |
| Provisional/unpublished | Cannot be searched; flag as residual risk |
| Concept abstraction | Search at multiple levels of abstraction (specific + general) |
| LLM hallucination | Never accept LLM-generated patent numbers without verification against real databases |

**The LLM hallucination risk:** Generic LLMs (ChatGPT, etc.) hallucinate patent numbers, assignees, and dates. For any AI-generated patent citation, always verify existence in a real patent database before relying on it.

**Recommended AI search architecture for agents:**
1. RAG (Retrieval-Augmented Generation) over actual patent database indices
2. LLM constrained to only reference indexed documents — never generate patent IDs
3. Two-stage pipeline: dense vector retrieval → LLM re-ranking and explanation
4. Full traceability: every cited document traceable to source database record

**Multi-database strategy for comprehensive coverage:**

```
Stage 1 (Keyword): USPTO PPS + Google Patents
Stage 2 (Classification): Espacenet CPC search + PatentScope IPC search
Stage 3 (Citation): INPADOC family + CCD forward/backward citations
Stage 4 (Semantic): PQAI or PatSnap semantic search
Stage 5 (NPL): Google Scholar + arXiv + domain-specific DBs
Stage 6 (Verification): Cross-check hits across databases; note gaps
```

**Human-in-the-loop requirements:**
- AI accelerates; human expert validates
- LLM summaries of patent claims should be verified against actual claim text
- Relevance judgments for §102/§103 require legal expertise
- GDPR and ethical compliance for data handling

**Agentic patent search loop:**
1. Generate initial query from description
2. Execute queries across multiple databases
3. Parse and deduplicate results
4. Identify most relevant results (semantic similarity + classification match)
5. Expand search via citations of top results
6. Expand CPC codes to adjacent classes
7. Check NPL for each primary concept
8. Synthesize findings; identify gaps
9. Generate follow-up queries for gaps
10. Iterate until coverage is sufficient (defined by: no new relevant results in 2 iterations)

---

## 7. Source URLs

All URLs fetched or referenced in this research:

### Search Syntax & Database Help
- https://worldwide.espacenet.com/patent/help/advanced-search-change-operators
- https://www.uspto.gov/patents/search/patent-public-search/faqs
- https://www.greyb.com/blog/patent-search/
- https://worldwide.espacenet.com/help?topic=classificationsearch&method=handleHelpTopic&locale=en_ep
- https://support.google.com/faqs/answer/7049475?hl=en
- https://www.uspto.gov/sites/default/files/documents/Patent-Public-Search-Search-overview-QRG.pdf
- https://worldwide.espacenet.com/patent/help/query-syntax-proximity-operator
- https://worldwide.espacenet.com/help?topic=operators&locale=en_EP&method=handleHelpTopic
- https://worldwide.espacenet.com/patent/help/query-syntax-searchable-fields
- https://www.guideforinventors.com/projects/project1/espacenet-instructions/
- https://www.uspto.gov/patents/search/patent-public-search/searchable-indexes
- https://www.uspto.gov/sites/default/files/documents/Advanced-search-overview-QRG-Patent-Public-Search.pdf
- https://www.uspto.gov/sites/default/files/documents/Search-field-conversion-PatFT-AppFT-QRG-Patent-Public-Search.pdf
- https://worldwide.espacenet.com/help?topic=fieldidentifier&locale=en_EP&method=handleHelpTopic
- https://worldwide.espacenet.com/patent/help/smart-search-expert-mode
- https://worldwide.espacenet.com/patent/help/query-syntax-patent-numbers
- https://www.uspto.gov/patents/search/patent-search-strategy
- https://www.uspto.gov/sites/default/files/documents/patent-7step-classification.pdf

### Classification Systems
- https://wipo-analytics.github.io/handbook/classification.html
- https://www.wipo.int/en/web/classification-ipc
- https://patentpc.com/blog/understanding-the-patent-classification-system
- https://open.forem.com/patentscanai/leveraging-cpc-and-ipc-codes-to-improve-searches-using-classification-in-patent-search-5e2h
- https://thelaw.institute/patents/international-patent-classification-ipc-system/
- https://patentclassificationexplorer.com/
- https://www.upcounsel.com/classification-of-patents
- https://www.wipo.int/edocs/pubdocs/en/wipo-guide-ipc-2025-en-guide-to-the-international-patent-classification-2025.pdf
- https://guides.library.queensu.ca/patents/classification
- https://www.uspto.gov/patents/search/classification-standards-and-development

### Prior Art Search Strategies
- https://learn.library.wisc.edu/patents/lesson-4/
- https://dev.to/patentscanai/how-to-use-google-patents-for-prior-art-invalidity-searches-43pd
- https://dev.to/patentscanai/prior-art-search-for-patent-litigation-defense-a-strategic-guide-2g5p
- https://dev.to/patentscanai/ensuring-no-prior-art-is-overlooked-thorough-prior-art-search-techniques-2oa7
- https://dev.to/patentscanai/advanced-prior-art-search-strategies-for-ip-professionals-5dn
- https://guides.library.cmu.edu/patentsearch
- https://parolaanalytics.com/guide/prior-art-search-guide/
- https://worldwide.espacenet.com/patent/help/inpadocfamily
- https://worldwide.espacenet.com/help?topic=patentfamily&locale=en_EP&method=handleHelpTopic
- https://wipo-analytics.github.io/handbook/citations.html

### Non-Patent Literature
- https://www.dexpatent.com/npl-search/
- https://greyb.com/blog/non-patent-literature-search-databases/
- https://www.uspto.gov/learning-and-resources/support-centers/scientific-and-technical-information-center-stic/electronic
- https://www.patlytics.ai/blog/non-patent-literature-what-it-is-and-how-it-works
- https://open.forem.com/patentscanai/how-to-search-non-patent-literature-for-prior-art-4ag8

### Historical and Niche Patent Finding
- https://libguides.nypl.org/patents/historical_patents
- https://www.lib.ncsu.edu/ptrc/search/patentshistorical
- https://www.wipo.int/en/web/patents/historical_patents
- https://ipwatchdog.com/obscure-patents/

### APIs — USPTO
- https://developer.uspto.gov/api-catalog
- https://data.uspto.gov/apis/patent-file-wrapper/search
- https://data.uspto.gov/
- https://patentsview.org/apis/purpose
- https://patentsview.org/apis/api-endpoints
- https://www.patentclaimmaster.com/blog/integration-with-uspto-open-data-portal/

### APIs — EPO / Espacenet
- https://ops.epo.org/
- https://developers.epo.org/
- https://pypi.org/project/python-epo-ops-client/
- https://github.com/abdullahatrash/epo-ops-sdk/blob/main/OPS.md
- https://github.com/ip-tools/python-epo-ops-client
- https://docs.ip-tools.org/patzilla/datasource/epo-ops.html
- https://projectpq.ai/best-patent-search-apis-2025/

### APIs — Google Patents / SerpAPI
- https://serpapi.com/google-patents-api
- https://serpapi.com/google-patents-details-api
- https://serpapi.com/blog/scraping-google-patents-with-python-and-serpapi/
- https://github.com/SoftwareStartups/google-patents-mcp
- https://www.searchapi.io/google-patents

### APIs — Lens.org
- https://about.lens.org/
- https://support.lens.org/knowledge-base/lens-patent-and-scholar-api/
- https://docs.api.lens.org/
- https://github.com/cambialens/lens-api-doc/blob/master/patent-api-doc.md
- https://api.lens.org/swagger-ui.html

### APIs — WIPO PatentScope
- https://www.wipo.int/en/web/patentscope
- https://patentscope.wipo.int/
- https://www.wipo.int/en/web/patentscope/data/index
- https://apicatalog.wipo.int/
- https://wipo-analytics.github.io/manual/databases.html
- https://wipo-analytics.github.io/manual/patentscope-1.html

### APIs — Commercial
- https://www.patsnap.com/
- https://clarivate.com/ (Derwent Innovation)
- https://projectpq.ai/ (PQAI)
- https://softwarefinder.com/artificial-intelligence/patsnap

### PCT / International
- https://www.wipo.int/en/web/pct-system
- https://www.uspto.gov/patents/basics/international-protection/patent-cooperation-treaty
- https://www.wipo.int/edocs/mdocs/africa/en/ompi_pct_cas_18/ompi_pct_cas_18_t_3.pdf
- https://www.wipo.int/edocs/mdocs/pct/en/ompi_pct_yoa_19/ompi_pct_yao_19_t4.pdf
- https://en.wikipedia.org/wiki/Patent_Cooperation_Treaty
- https://globaldossier.uspto.gov/

### LLM / AI Patent Search
- https://arxiv.org/html/2403.04105v2
- https://www.sciencedirect.com/science/article/abs/pii/S0172219025000080
- https://www.ischool.berkeley.edu/projects/2025/ai-patent-intelligence-copilot
- https://relecura.ai/the-future-of-patent-searching-leveraging-llms-for-comprehensive-results/
- https://link.springer.com/article/10.1007/s10462-025-11168-z
- https://www.patlytics.ai/blog/ai-patent-search
- https://www.sciencedirect.com/science/article/pii/S1474034625001946
- https://saastake.com/top-ai-patent-search-tools/
- https://arxiv.org/pdf/1911.11069
- https://www.mdpi.com/2079-8954/13/4/259
- https://projectpq.ai/top-ai-patent-search-tools/
- https://www.tprinternational.com/2022-state-of-artificial-intelligence-ai-and-patent-searching/
