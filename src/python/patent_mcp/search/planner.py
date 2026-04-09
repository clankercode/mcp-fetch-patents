"""Natural language search planner — turns plain English into patent search query variants.

No LLM dependency. Uses keyword extraction, a static synonym table, and template-based
query generation to expand a single description into multiple search formulations.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field


# ---------------------------------------------------------------------------
# Stop words — filtered out during concept extraction
# ---------------------------------------------------------------------------

_STOP_WORDS: set[str] = {
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
    "of", "with", "by", "from", "as", "is", "was", "are", "were", "be",
    "been", "being", "have", "has", "had", "do", "does", "did", "will",
    "would", "shall", "should", "may", "might", "can", "could", "must",
    "about", "above", "after", "before", "between", "into", "through",
    "during", "against", "without", "within", "along", "across", "behind",
    "below", "beneath", "beside", "beyond", "under", "until", "upon",
    "that", "this", "these", "those", "which", "who", "whom", "whose",
    "what", "where", "when", "why", "how", "each", "every", "all", "any",
    "both", "few", "more", "most", "other", "some", "such", "no", "nor",
    "not", "only", "own", "same", "so", "than", "too", "very", "just",
    "also", "then", "there", "here", "now", "it", "its", "they", "them",
    "their", "we", "us", "our", "you", "your", "he", "him", "his", "she",
    "her", "my", "me", "i", "using", "use", "used", "based", "related",
    "new", "novel", "improved", "existing", "like", "similar", "etc",
    "specifically", "particularly", "especially", "generally", "typically",
    "works", "working", "work", "make", "makes", "made", "find", "look",
    "looking", "search", "patent", "patents", "invention", "prior", "art",
}


# ---------------------------------------------------------------------------
# Synonym table — phrase/term → list of alternatives for patent language
# ---------------------------------------------------------------------------

_SYNONYMS: dict[str, list[str]] = {
    # Power / Energy
    "wireless charging": ["inductive coupling", "contactless power transfer", "wireless power transfer", "inductive power transfer"],
    "wireless power": ["contactless power", "inductive power", "wireless energy transfer"],
    "battery": ["energy storage device", "electrochemical cell", "rechargeable cell", "accumulator"],
    "solar cell": ["photovoltaic cell", "solar panel", "photovoltaic device", "PV cell"],
    "solar": ["photovoltaic", "solar energy", "solar radiation"],
    "fuel cell": ["electrochemical energy converter", "hydrogen fuel cell"],
    "capacitor": ["energy storage element", "charge storage device"],
    "supercapacitor": ["ultracapacitor", "electrochemical capacitor", "double-layer capacitor"],
    "transformer": ["magnetic core", "inductive device", "voltage converter"],
    "inverter": ["power converter", "DC-AC converter"],

    # Computing / AI
    "machine learning": ["artificial intelligence", "neural network", "deep learning", "pattern recognition"],
    "neural network": ["deep learning model", "artificial neural network", "ANN"],
    "computer vision": ["image recognition", "visual processing", "image analysis", "object detection"],
    "natural language processing": ["NLP", "text analysis", "language understanding", "computational linguistics"],
    "blockchain": ["distributed ledger", "decentralized ledger", "cryptographic chain"],
    "cloud computing": ["distributed computing", "remote computing", "network computing"],
    "processor": ["CPU", "computing unit", "microprocessor", "processing element"],
    "memory": ["storage device", "data storage", "RAM", "cache memory"],
    "algorithm": ["computational method", "data processing method"],
    "encryption": ["cryptography", "cipher", "encoding", "data security"],
    "database": ["data store", "data repository", "data management system"],

    # Manufacturing
    "3d printing": ["additive manufacturing", "rapid prototyping", "three-dimensional printing", "fused deposition modeling"],
    "robot": ["robotic system", "automated manipulator", "robotic device"],
    "robotic": ["automated", "autonomous", "mechanized"],
    "sensor": ["detector", "transducer", "sensing element", "measuring device"],
    "actuator": ["drive mechanism", "motor", "activating element"],
    "laser": ["coherent light source", "optical amplifier", "laser beam"],
    "welding": ["joining", "bonding", "fusion bonding"],
    "mold": ["mould", "die", "casting form"],
    "cnc": ["computer numerical control", "numerically controlled"],

    # Medical / Bio
    "drug delivery": ["pharmaceutical delivery", "therapeutic delivery", "controlled release", "drug administration"],
    "medical device": ["biomedical device", "clinical device", "therapeutic apparatus"],
    "implant": ["prosthesis", "prosthetic device", "biocompatible implant"],
    "stent": ["vascular scaffold", "endovascular implant", "tubular implant"],
    "catheter": ["intravascular device", "tubular medical device"],
    "antibody": ["immunoglobulin", "monoclonal antibody"],
    "protein": ["polypeptide", "amino acid sequence"],
    "dna": ["nucleic acid", "polynucleotide", "genetic material"],
    "gene therapy": ["genetic therapy", "gene transfer", "gene editing"],
    "diagnostic": ["detection method", "assay", "screening"],

    # Materials
    "composite": ["composite material", "fiber-reinforced material", "laminate"],
    "polymer": ["plastic", "resin", "thermoplastic", "synthetic resin"],
    "semiconductor": ["integrated circuit", "chip", "transistor", "silicon device"],
    "nanoparticle": ["nanomaterial", "nano-sized particle", "nanostructure"],
    "coating": ["surface treatment", "film", "layer", "surface coating"],
    "alloy": ["metal composition", "metallic mixture"],
    "ceramic": ["sintered material", "oxide material"],
    "graphene": ["carbon nanostructure", "two-dimensional carbon"],

    # Transport
    "autonomous vehicle": ["self-driving vehicle", "driverless vehicle", "automated driving system"],
    "electric vehicle": ["EV", "electric car", "battery electric vehicle", "electric motor vehicle"],
    "lidar": ["light detection and ranging", "laser scanner", "optical radar"],
    "radar": ["radio detection and ranging", "microwave sensor"],

    # Communication
    "antenna": ["aerial", "radiator", "electromagnetic radiator"],
    "wireless": ["radio frequency", "RF", "electromagnetic", "over-the-air"],
    "optical fiber": ["fibre optic", "optical waveguide", "light guide"],
    "5g": ["fifth generation", "new radio", "NR", "mmWave"],
    "bluetooth": ["short-range wireless", "personal area network"],

    # Mechanical / structural
    "valve": ["flow control device", "gate valve", "control element"],
    "bearing": ["rotational support", "journal bearing", "bushing"],
    "spring": ["elastic element", "resilient member", "biasing element"],
    "gear": ["toothed wheel", "transmission element", "cogwheel"],
    "seal": ["gasket", "sealing element", "O-ring"],
    "hinge": ["pivot", "articulation", "rotary joint"],
    "filter": ["filtration device", "separation element", "strainer"],
    "pump": ["fluid mover", "compressor", "fluid displacement device"],
    "heat exchanger": ["thermal exchanger", "heat transfer device", "radiator"],
    "turbine": ["rotary engine", "turbo machine"],

    # Optics / Display
    "display": ["screen", "monitor", "visual display", "panel"],
    "led": ["light emitting diode", "solid-state light", "electroluminescent device"],
    "oled": ["organic light emitting diode", "organic electroluminescent"],
    "lens": ["optical element", "refractive element"],
    "camera": ["image sensor", "imaging device", "image capture device"],

    # General patent language
    "method": ["process", "technique", "procedure"],
    "device": ["apparatus", "system", "equipment", "mechanism"],
    "coupled": ["connected", "attached", "linked", "joined", "fastened"],
    "disposed": ["positioned", "arranged", "located", "situated"],
    "adjacent": ["proximate", "near", "neighboring", "abutting"],
    "layer": ["film", "coating", "stratum"],
    "surface": ["face", "exterior", "outer surface"],
    "housing": ["enclosure", "casing", "chassis", "body"],
    "opening": ["aperture", "orifice", "hole", "port"],
    "channel": ["conduit", "passage", "duct", "groove"],
    "substrate": ["base", "foundation", "support layer"],
    "controller": ["control unit", "control module", "processor"],
    "signal": ["data signal", "electrical signal", "communication signal"],
    "circuit": ["electronic circuit", "circuitry", "electrical circuit"],
    "module": ["unit", "component", "assembly"],
    "interface": ["connection", "port", "coupling"],
}


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------

@dataclass
class QueryVariant:
    """A single search query generated by the planner."""
    query: str
    variant_type: str   # "broad", "synonym_expanded", "title_focused", "quoted_phrase", "concepts_and"
    backend: str        # "google_patents" | "serpapi" | "any"
    rationale: str


@dataclass
class SearchIntent:
    """Structured output from the planner — everything needed to drive a multi-query search."""
    raw_description: str
    concepts: list[str]
    synonyms: dict[str, list[str]]
    exclusions: list[str]
    date_cutoff: str | None
    jurisdictions: list[str]
    query_variants: list[QueryVariant]
    rationale: str


# ---------------------------------------------------------------------------
# Planner
# ---------------------------------------------------------------------------

class NaturalLanguagePlanner:
    """Turn a plain-English invention description into multiple patent search queries."""

    def plan(
        self,
        description: str,
        date_cutoff: str | None = None,
        jurisdictions: list[str] | None = None,
    ) -> SearchIntent:
        concepts = self._extract_concepts(description)
        synonyms = self._expand_synonyms(concepts)
        variants = self._generate_variants(
            description, concepts, synonyms, date_cutoff, jurisdictions,
        )
        rationale = self._build_rationale(concepts, synonyms)

        return SearchIntent(
            raw_description=description,
            concepts=concepts,
            synonyms=synonyms,
            exclusions=[],
            date_cutoff=date_cutoff,
            jurisdictions=jurisdictions or [],
            query_variants=variants,
            rationale=rationale,
        )

    # ------------------------------------------------------------------
    # Concept extraction
    # ------------------------------------------------------------------

    def _extract_concepts(self, description: str) -> list[str]:
        """Extract key concepts: multi-word phrases first, then important single words."""
        text = description.lower().strip()
        found_phrases: list[str] = []

        # Match multi-word phrases from synonym table (longest first)
        # Use word-boundary matching to avoid false positives like
        # "non-electric vehicle" matching "electric vehicle"
        for phrase in sorted(_SYNONYMS, key=len, reverse=True):
            if " " not in phrase:
                continue
            pattern = r"(?<!\w)" + re.escape(phrase) + r"(?!\w)"
            if re.search(pattern, text):
                found_phrases.append(phrase)
                text = re.sub(pattern, " _ ", text, count=1)

        # Tokenise remaining text — allow leading digits for terms like "5g"
        words = re.findall(r"\b[a-z0-9][a-z0-9-]*[a-z0-9]\b", text)
        single_words = [w for w in words if w not in _STOP_WORDS and w != "_" and len(w) > 1]

        # Also check single-word synonym keys against the original text
        # (catches terms like "5g" that the tokenizer might miss)
        for key in _SYNONYMS:
            if " " in key:
                continue
            pattern = r"(?<!\w)" + re.escape(key) + r"(?!\w)"
            if re.search(pattern, description.lower()) and key not in found_phrases:
                single_words.append(key)

        # Promote single words that appear in synonym table
        promoted: list[str] = []
        remainder: list[str] = []
        for w in single_words:
            if w in _SYNONYMS:
                promoted.append(w)
            else:
                remainder.append(w)

        # Deduplicate while preserving order
        seen: set[str] = set()
        result: list[str] = []
        for c in found_phrases + promoted + remainder:
            if c not in seen:
                seen.add(c)
                result.append(c)
        return result

    # ------------------------------------------------------------------
    # Synonym expansion
    # ------------------------------------------------------------------

    def _expand_synonyms(self, concepts: list[str]) -> dict[str, list[str]]:
        """Look up each concept in the synonym table."""
        out: dict[str, list[str]] = {}
        for concept in concepts:
            alts = _SYNONYMS.get(concept, [])
            if alts:
                out[concept] = list(alts)
        return out

    # ------------------------------------------------------------------
    # Query variant generation
    # ------------------------------------------------------------------

    def _generate_variants(
        self,
        description: str,
        concepts: list[str],
        synonyms: dict[str, list[str]],
        date_cutoff: str | None,
        jurisdictions: list[str] | None,
    ) -> list[QueryVariant]:
        variants: list[QueryVariant] = []

        if not description.strip():
            return variants

        # 1. Broad — raw description as-is
        variants.append(QueryVariant(
            query=description.strip(),
            variant_type="broad",
            backend="any",
            rationale="Raw description for maximum recall",
        ))

        # 2. Synonym-expanded — OR groups for concepts with known synonyms
        if synonyms:
            parts: list[str] = []
            for concept in concepts:
                alts = synonyms.get(concept)
                if alts:
                    # Build OR group: ("wireless charging" OR "inductive coupling" OR ...)
                    options = [f'"{concept}"'] + [f'"{a}"' for a in alts[:3]]
                    parts.append(f"({' OR '.join(options)})")
                else:
                    parts.append(concept)
            if parts:
                q = " AND ".join(parts[:6])  # cap at 6 groups to keep query manageable
                variants.append(QueryVariant(
                    query=q,
                    variant_type="synonym_expanded",
                    backend="any",
                    rationale="Synonym expansion for broader coverage",
                ))

        # 3. Title-focused — core concepts in title search (Google Patents syntax)
        if concepts:
            # Pick the 2-3 most important concepts for title search
            core = concepts[:3]
            title_parts = " ".join(f'"{c}"' if " " in c else c for c in core)
            variants.append(QueryVariant(
                query=title_parts,
                variant_type="title_focused",
                backend="any",
                rationale="Core concepts only — tighter precision",
            ))

        # 4. Quoted multi-word phrases — exact match for key phrases
        multi_word = [c for c in concepts if " " in c]
        if multi_word:
            quoted = " AND ".join(f'"{p}"' for p in multi_word[:3])
            remaining_single = [c for c in concepts if " " not in c][:3]
            if remaining_single:
                quoted += " " + " ".join(remaining_single)
            variants.append(QueryVariant(
                query=quoted,
                variant_type="quoted_phrase",
                backend="any",
                rationale="Exact multi-word phrase matching",
            ))

        # 5. Concepts AND-linked — all single concepts joined
        if len(concepts) >= 2:
            and_query = " AND ".join(concepts[:6])
            variants.append(QueryVariant(
                query=and_query,
                variant_type="concepts_and",
                backend="any",
                rationale="All key concepts required",
            ))

        return variants

    # ------------------------------------------------------------------
    # Rationale
    # ------------------------------------------------------------------

    def _build_rationale(
        self,
        concepts: list[str],
        synonyms: dict[str, list[str]],
    ) -> str:
        parts = [f"Extracted {len(concepts)} concepts: {', '.join(concepts[:8])}"]
        if synonyms:
            expanded = [k for k in synonyms]
            parts.append(f"Synonym expansion available for: {', '.join(expanded[:5])}")
        return ". ".join(parts) + "."
