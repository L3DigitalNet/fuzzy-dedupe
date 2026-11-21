---
spec_id: SPEC-FZDP
title: 'Fuzzy DeDupe'
status: draft
profile: standard
owner: 'L3DigitalNet'
implementer: 'Fuzzy DeDupe implementation team'
created: '2026-11-19'
last_reviewed: '2026-11-21'
supersedes: null
superseded_by: null
related:
  adrs:
    - 'adr-0001-fuzzy-dedupe-use-egui-for-the-v1-desktop-ui'
  tickets: []
  repositories: []
  prior_specs: []
---

# Fuzzy DeDupe — Product and Architecture Specification

Draft · Standard profile · SPEC-FZDP

The owner has approved the product name, independent Rust implementation, license, plain-text-only first release, Linux-only first release, and compact desktop interaction model. The technical design and release gates below remain proposals unless identified as confirmed.

[Requirements](#7-requirements) · [Architecture](#8-architecture-and-design) · [UI handoff](#11-ui-pages--api-endpoints) · [Cleanup safety](#12-error-handling-and-recovery) · [Verification matrix](#173-requirement-to-test-traceability) · [Milestones](#19-implementation-plan) · [Open decisions](#21-open-questions-and-decisions)

## Revision History

| Version | Date | Change |
| --- | --- | --- |
| 0.2 | 2026-11-11 | Recorded the approved compact desktop design and Figma handoff. |
| 0.3 | 2026-11-19 | Recorded the project name, GPL-3.0-only license, and from-scratch implementation. |
| 0.4 | 2026-11-19 | Converted v0.3 to the Standard profile, assigned requirement IDs, and linked proposed verification to those IDs. |
| 0.5 | 2026-11-21 | Answered OQ-002 (plain-text-only V1) and OQ-003 (Linux-only V1, portable to macOS and Windows later); added C-013, C-014, WH-003, WH-004 and NFR-008. |
| 0.6 | 2026-11-21 | Answered OQ-004: egui/eframe selected by ADR 0001, with Slint as the fallback. Added NG-003 (no assistive-technology support) and narrowed NFR-005 to keyboard operation and display scaling. |

**Spec lifecycle:** This document is **living until `approved`**, then **change-controlled**: post-approval edits require a new revision row and, for scope-affecting changes, re-approval by the owner. Implementation deviations are recorded in the [Deviations Log](#deviations-log), not silently patched into requirements. When replaced, set `status: superseded` and `superseded_by:` in the frontmatter.

## 1. Purpose & Background

The initiating workflow involved finding files by content and acting on them from one interface. Investigation of document search tools, file managers, and their plugins did not establish a satisfactory integrated solution for the desired fuzzy text cleanup workflow. This is the project's motivation, not a claim that no competing product exists.

A useful application must help answer four questions: which files are the same, which are nearly the same, what differs, and which files the user intends to retain. Recovering disk space is useful, but organizing redundant document collections is equally important.

### Primary use cases

| Use case | Expected outcome |
| --- | --- |
| Repeated downloads or copies | Identify identical documents under unrelated filenames and paths. |
| Multiple revisions | Find documents with additions, deletions, or substitutions and expose the unique text in each. |
| Reorganized notes | Find overlap despite moved paragraphs or sections. |
| Consolidated documents | Recognize that a smaller document is substantially contained within a larger document. |
| Shared templates | Avoid presenting common headers, disclaimers, or boilerplate as compelling duplicate evidence. |
| Repeated cleanup sessions | Reuse valid prior analysis while checking whether source files changed. |

The user reviews pairwise evidence in Findings and Compare before approving any file operation. The approved design and its acceptance checks are in §11.

## 2. Scope

### 2.1 In Scope

| Capability | Status |
| --- | --- |
| Desktop GUI and Rust implementation | Confirmed direction |
| Text-content fuzzy duplicate discovery | Confirmed core direction |
| Exact duplicate detection | Recommended core; strongly implied |
| Configurable similarity thresholds and visible scores | Recommended core |
| Compact table-dominant findings and large read-only side-by-side comparison | Approved UI direction; implementation contract in §11 |
| Settings, scan setup, and cleanup popups | Approved compact design; no dedicated main screens for these tasks |
| Selecting retained files and deleting unwanted copies | Recommended core |
| Bulk actions with preview, revalidation, and failure reporting | Recommended core |
| Multiple scan roots, recursion, filters, and exclusions | Recommended core |
| Persistent fingerprints and incremental rescans | Recommended architecture |
| Rename, move, and copy | Optional V1 extension; not required for the essential workflow |
| Opening a file or containing folder externally | Recommended convenience |
| Built-in editing or merging | Excluded |
| Semantic/AI similarity, embeddings, language models | Excluded from the planned product |
| Image, audio/music, and video analysis | Later releases |
| Full-text search index or live filesystem watching | Deferred |
| Public CLI/API, dynamic plugins, distributed scanning | Deferred unless separately approved |
| Plain-text files as the only V1 input format | Confirmed (C-013) |
| Linux as the only V1 release platform, with an architecture portable to macOS and Windows | Confirmed (C-014) |

**Confirmed V1 boundary: decodable plain-text files (C-013).** Examples include TXT, Markdown, source files, configuration files, and text logs. V1 analyzes these files as text, not parsing or executing them as programs.

The owner confirmed that V1 is plain text only. PDF, DOCX, ODT, RTF, and other containers requiring extraction are excluded from V1 and deferred (WH-003); V1 includes no extractors for them.

HTML and structured text can be read literally in V1. Rendering HTML, stripping markup, understanding source-code syntax, interpreting configuration semantics, and normalizing structured records are separate enhancements. They must not be implied merely because the file is readable.

Archive traversal, OCR, encrypted-document access, and extraction from embedded attachments are outside the recommended V1 baseline. The generic exact-hash service may be media-independent internally, but that is not authorization to expand V1's user-facing supported-file scope.

### 2.2 Out of Scope (Non-Goals — never)

| ID | Non-goal | Reason |
| --- | --- | --- |
| NG-001 | Edit or merge source documents. | The approved review workflow is read-only. |
| NG-002 | Infer semantic similarity with embeddings or language models. | The product compares observable text content using conventional algorithms. |
| NG-003 | Support assistive technologies such as screen readers. | Owner decision (2026-11-21): the project provides no disability or screen-reader support. Keyboard operation and display scaling remain ordinary usability requirements (NFR-005). |

### 2.3 Won't Have in v1 (deferred — not never)

| ID | Deferred capability | Revisit when |
| --- | --- | --- |
| WH-001 | Image, audio, and video analysis. | The text workflow is validated and the next analyzer meets the expansion gate in §8.8. |
| WH-002 | Public CLI, dynamic plugins, and distributed scanning. | A separate product decision establishes a need. |
| WH-003 | Extracted document formats, including PDF, DOCX, ODT, and RTF. | The plain-text workflow is validated and a separate product decision adds an extractor. |
| WH-004 | macOS and Windows release support, including their packaging, signing, and notarization. | The Linux V1 is validated and the owner selects the next platform. |

The proposed order is correctness and data safety, clarity of match evidence, usable review, bounded resource consumption, and then raw throughput. All analysis should work locally without accounts, remote services, telemetry, model downloads, or internet connectivity.

The baseline should support local folders and OS-mounted filesystems. Native cloud-provider APIs, remote scan agents, and a centrally shared database are not required.

### 2.4 Boundaries

This is a **content-aware duplicate finder with read-only review and controlled file cleanup**. It is not a general-purpose document search engine, editor, document-merging application, knowledge-management system, or automated “delete anything similar” utility.

External opening remains compatible with the design: users may open a document in their preferred application, then return and refresh the scan. That does not require embedding an editor.

## 3. Context

### 3.1 Current State

This document consolidates the conversation and supersedes earlier suggestions that included editing, semantic analysis, or the now-rejected multi-screen/group-card interface. The owner-approved UI has **a dominant grouped findings table and a separate, dominant side-by-side comparison workspace**, with compact popup windows for supporting tasks. [UX01], [UX03]

Product scope and approval status come from the conversation. Technical references were recorded during the original November 11, 2026 research; the compact Figma nodes, typography, dimensions, and semantic colors were inspected on that date for version 0.2. Version 0.3 records the new project decisions and checks the GPLv3/SPDX references on November 19, 2026; it does not revalidate the GUI or dependency stack. A linked, editable design prototype exists; no executable application, deletion-safety validation, or performance benchmark is represented as completed. Technical decisions remain recommendations unless explicitly identified as confirmed.

### 3.2 Target State

Build **Fuzzy DeDupe**, a desktop GUI application that discovers exact duplicates and substantially similar text documents, explains their relationships through read-only comparison, and lets the user safely remove unwanted files without leaving the application.

Implement the application from scratch in Rust under **GPLv3 (`GPL-3.0-only`)**. dupeGuru remains a reference for product behavior and workflows, rather than the source-code foundation. GPLv3 is an explicit project choice. The implementation should follow this specification and the approved UI, using reviewed, compatible third-party dependencies where appropriate.

The product's distinguishing capability is **fuzzy matching of actual text content**, independent of filenames. It should find minor revisions, documents with rearranged passages, and substantial partial overlap—not merely files with similar names or identical hashes. Matching will use conventional, non-AI algorithms. There will be no semantic similarity, embeddings, or language-model dependency.

The first release is text-only. It does not include an editor, merging, or image/audio/video analysis. Later media support should reuse the same discovery, job management, persistence, results, and cleanup infrastructure while supplying its own decoding, fingerprinting, candidate retrieval, scoring, and preview implementations.

The recommended foundation is **Rust + egui/eframe + SQLite**, with **BLAKE3** for exact-match candidate identification, **Rayon and bounded channels** for background work, and a separately testable text-analysis engine. Shingles, MinHash/LSH, and local passage fingerprints are algorithm candidates to evaluate rather than unquestioned final selections.

Safety and evidence take precedence over aggressive cleanup. A high fuzzy score is not proof that a file can be deleted. The application must distinguish byte equality, equality after normalization, overall resemblance, and partial containment; preserve visibility into unique content; and revalidate files before acting on them.

### 3.3 Assumptions

The plain-text-only V1 format boundary and the Linux-only V1 release are owner-confirmed (C-013, C-014). egui/eframe is the selected UI framework ([ADR 0001](../../adr/adr-0001-use-egui-for-the-v1-desktop-ui.md)), final once the MS-1 prototype passes. The following remain proposed: SQLite, specific crates, Linux package formats, similarity metrics, grouping policy, persistent caches, and rename/move/copy operations. Compact table-first findings, prominent side-by-side comparison, and popup-based supporting tasks are now approved UI direction, not unresolved layout proposals.

Exact duplicate detection is strongly implied by the application's purpose. Side-by-side comparison is owner-confirmed; safe bulk deletion remains a core recommendation supporting the approved cleanup flow. More extensive file management is optional scope, not a reason to delay a useful first release. The specific layout measurements and state contracts in §11 are implementation guidance for the approved design, not evidence that they have been tested in a working application.

Throughout the technical sections, **must** identifies a condition of the recommended implementation baseline. It does not turn every recommendation into a previously approved user requirement. Confirmed exclusions take precedence over all proposed features.

### 3.4 Constraints

| ID | Direction established in the conversation |
| --- | --- |
| C-001 | Build a standalone GUI application rather than depend on Double Commander or Krusader plugins. |
| C-002 | Write a new Rust application from scratch with expanded fuzzy text-content detection. The earlier dupeGuru clone/port/adaptation proposal is superseded. |
| C-003 | V1 supports text documents only. Images, video, music, and other media belong to later releases. |
| C-004 | Built-in file editing is not needed and is excluded from the planned application. |
| C-005 | Semantic/AI similarity is excluded. Do not silently reintroduce embeddings or model-based analysis in the roadmap. |
| C-006 | Finding and cleaning up duplicate or near-duplicate files within the GUI remains the central workflow. |
| C-007 | The product name is Fuzzy DeDupe. |
| C-008 | Findings after a scan must appear in a table that is the major element of the findings workspace. |
| C-009 | Read-only side-by-side comparison panes must be large and prominent while comparing files. |
| C-010 | Use compact, streamlined controls and fewer main screens; use popup windows for settings. |
| C-011 | The updated Figma compact-desktop design is the approved visual direction: Findings and Compare workspaces, with scan setup and cleanup also presented as popups. |
| C-012 | Use GNU GPL version 3.0. This specification records the license as `GPL-3.0-only`, without an assumed grant for later versions. |
| C-013 | V1 analyzes plain-text files only. Extracted document formats such as PDF, DOCX, ODT, and RTF are not part of V1. |
| C-014 | V1 is released for Linux only. Develop it so that macOS and Windows support can be added later without redesigning the domain, analysis, storage, or cleanup-safety model. |

The from-scratch implementation is a confirmed development decision, not a claim that a repository, codebase, or provenance audit already exists. dupeGuru may inform product research, but its source, tests, documentation, and bundled assets are not the starting implementation. Normal third-party libraries remain eligible after dependency and license review.

## 4. Goals

| ID | Goal | Success signal | Achieved by |
| --- | --- | --- | --- |
| G-001 | Find copies and related text across folders. | The labeled corpus distinguishes byte equality, normalized equality, resemblance, and containment. | FR-007 through FR-014 |
| G-002 | Explain why two files match. | A reviewer can locate shared passages and unique original text side by side. | FR-015 through FR-019 |
| G-003 | Remove only files the user intends to remove. | Retained copies survive and the operation journal accounts for every target, including partial failures. | FR-020 through FR-024 |

> **§5 (Stakeholders and Users) is Full-tier** and is intentionally omitted at the Standard profile.

## 6. Glossary

| Term | Meaning in this product |
| --- | --- |
| Exact duplicate | Files whose original bytes are equal; distinguish hash-matched candidates from byte-verified equality. |
| Normalized-text equality | Decoded text becomes equal under an explicitly identified normalization profile. Original files may differ. |
| Near-duplicate | Substantial textual resemblance under a stated metric, not a claim of identical meaning. |
| Containment | Much of one document's text appears in another; inherently directional when reported per document. |
| Shared passage | A matching span inside otherwise different documents. |
| Similarity score | A measurement produced by a specific algorithm and profile, not a probability that deletion is safe. |
| Comparison reference / retained copy | A reference anchors pairwise evidence; protection and planned retention are separate states. None establishes an algorithmically “best” document. |
| Deduplication | Finding and removing unwanted copies; V1 does not promise filesystem-level block deduplication. |

## 7. Requirements

> **Quality rule:** Each requirement is one testable statement with a stable ID, a rationale, an acceptance criterion, and a priority. Priorities: **Must** (release-blocking), **Should** (important, briefly deferrable), **Could** (nice-to-have, must not delay release). Anything "Won't" belongs in §2.3, not here.

The source labels in §7.1 (for example, DISC 01 for the first discovery row) identify the original V1 rows in the Fuzzy DeDupe product and architecture specification v0.3. The FR IDs are their canonical references. Each row retains its original acceptance condition.

The owner confirmed fuzzy discovery from text content, cleanup within the GUI, and the compact Findings and Compare workspaces with read-only paired panes and supporting popups. Exact matching is strongly implied and recommended. The detailed discovery, scoring, grouping, retention, bulk cleanup and safety rules form the recommended implementation baseline. Layout measurements and state contracts guide implementation of the approved design.

Priority applies within that baseline. **Must** denotes a proposed release gate and **Should** an important, deferrable capability. Interface and data contracts are Must within the proposed baseline. The UI framework is selected by ADR 0001. Store, matching algorithms, default thresholds, numerical performance budgets and exposure of permanent deletion remain open choices.

### 7.1 Functional Requirements

| ID | Requirement | Rationale | Acceptance Criteria | Priority |
| --- | --- | --- | --- | --- |
| FR-001 | The system shall scan the user’s selected directory trees recursively as one search scope. | Collections can span roots or include a nested root twice. (DISC 01) | Cross-root duplicates are found and overlapping roots do not duplicate the same entry. | Should |
| FR-002 | The system shall apply user-selected extension, path, size and date filters and explicit exclusions. | Users need to explain a scan’s coverage. (DISC 02) | The scan summary records the effective filters and reasons for skipped files. | Should |
| FR-003 | The system shall expose and apply configurable hidden-file and ignore-file policies. | Traversal defaults can omit wanted files. (DISC 03) | Relevant files are not silently excluded by library defaults. | Should |
| FR-004 | The system shall skip non-regular special files and leave directory symlinks unfollowed by default. | Special nodes can block reads and symlinks can create cycles. (DISC 04) | Loop, pipe, socket, and device fixtures cannot hang traversal. | Must |
| FR-005 | The system shall run cancellable scans off the UI thread. | Users need to keep working while a scan runs. (DISC 05) | The GUI remains responsive and interrupted work is labeled incomplete. | Must |
| FR-006 | The system shall record discovery problems against each affected file. | Per-file issues distinguish unreadable inputs from analyzed nonmatches. (DISC 06) | Problems are recorded per file rather than silently reported as successful coverage. | Must |
| FR-007 | The system shall group byte-identical files across filenames after verifying the original bytes. | An exact label supports retention decisions. (MATCH 01) | Renamed copies appear in the same verified group. | Must |
| FR-008 | The system shall label normalized-text equality separately from original-byte equality. | Decoding and normalization can erase byte-level differences. (MATCH 02) | Encoding/format normalization cannot produce an “identical original files” label. | Must |
| FR-009 | The system shall retrieve near-duplicate text by its content independently of the filename. | Renamed revisions are a central use case. (MATCH 03) | Renamed, lightly edited examples are retrieved. | Must |
| FR-010 | The system shall detect substantial partial overlap and passages that survive section reordering. | A short document inside a longer one can have low overall resemblance. (MATCH 04) | Labeled containment and section-reordering cases are evaluated separately. | Must |
| FR-011 | The system shall expose score definitions and let users configure thresholds. | Scores need a metric and context to interpret them. (MATCH 05) | Each result records its analyzer, profile, metric, and verification level. | Should |
| FR-012 | The system shall make differences in the original decoded files available during review. | Unique text can affect which file the user keeps. (MATCH 06) | Normalization cannot hide unique text from the review interface. | Must |
| FR-013 | The system shall preserve pairwise evidence when presenting fuzzy groups. | Fuzzy similarity is nontransitive. (MATCH 07) | A chain of matches is not presented as all-pairs equivalence. | Must |
| FR-014 | The system shall report approximation and incomplete analysis coverage. | A missing result can reflect pruning or unfinished work. (MATCH 08) | Candidate caps, timeouts, decoding failures, and interrupted scans are visible. | Must |
| FR-015 | The system shall provide large read-only side-by-side panes in Compare. | Users need to inspect unique content before cleanup. (REVIEW 01) | Added, removed, and changed text is inspectable; panes dominate the working area. | Must |
| FR-016 | The system shall show the paths, sizes, timestamps, match type and score context of the files under review. | Similar names can refer to different targets. (REVIEW 02) | Users can identify the actual files targeted by an operation. | Should |
| FR-017 | The system shall support keyboard group navigation and explicit selection of files to retain. | Protection must follow the chosen file. (REVIEW 03) | Protected files remain protected across sorting and filtering. | Must |
| FR-018 | The system shall present Findings as a dominant compact grouped table keyed by stable row identities. | Actions must follow their targets as the view changes. (REVIEW 04) | Filtering, sorting, expansion, and returning from comparison preserve focus and queued intent. | Must |
| FR-019 | The system shall open Settings, New scan and Cleanup as popups over the current workspace. | Supporting tasks need to preserve review context. (REVIEW 05) | Supporting tasks preserve the underlying workspace rather than requiring separate pages. | Must |
| FR-020 | The system shall execute user-confirmed removal on the selected files and reflect the observed filesystem outcomes in Findings. | The cleanup workflow acts on actual copies. (CLEAN 01) | Filesystem outcomes and result-list changes agree. | Must |
| FR-021 | The system shall prefer Trash and require separate explicit authorization for any exposed permanent-delete action. | Trash rejection must preserve the user’s choice of action. (CLEAN 02) | Trash failure never silently falls back to permanent deletion. | Must |
| FR-022 | The system shall preview the concrete plan and revalidate its targets and retained copies before executing it. | Paths, identities and evidence can change after review. (CLEAN 03) | Changed or ambiguous targets are blocked or returned for review. | Must |
| FR-023 | The system shall require deliberate user selection and review before deleting a fuzzy match. | A score cannot establish that unique content is disposable. (CLEAN 04) | A threshold or default selection rule cannot autonomously remove fuzzy matches. | Must |
| FR-024 | The system shall record success, failure, skipped and uncertain outcomes per operation item. | A batch summary must account for partial execution. (CLEAN 05) | A partially completed batch cannot be reported as fully successful. | Must |

### 7.2 Non-Functional Requirements

| ID | Category | Requirement | Measurement / Acceptance Criteria | Priority |
| --- | --- | --- | --- | --- |
| NFR-001 | Resource bounds | The system shall bound worker queues, reads, analysis and preview work by the active resource policy. | Under load, work stays within the configured budgets, navigation remains available and a full progress queue still admits cancellation. | Must |
| NFR-002 | Accuracy | The system shall evaluate candidate retrieval separately from final classification on held-out document families. | Report retrieval recall and pruning, then final precision and recall by relation, profile and threshold. All labeled exact copies are correctly handled. | Must |
| NFR-003 | File safety | The system shall prevent wrong-target operations and silent destructive fallback in the cleanup safety suite. | The suite records zero wrong-target deletions and zero silent permanent deletions following Trash failure. | Must |
| NFR-004 | Privacy | The system shall support local scanning and comparison without an account, network access or default uploads. | With networking disabled, scanning and comparison complete. Default operation sends no text, paths, hashes, fingerprints, telemetry or crash reports. | Must |
| NFR-005 | Keyboard and scaling | The system shall keep Findings, Compare and popup controls operable with keyboard navigation and display scaling. | At supported scales, focus and actions remain reachable by keyboard and side labels distinguish changes without color. Assistive-technology support is a non-goal (NG-003). | Must |
| NFR-006 | Performance measurement | The system shall characterize cold and incremental scan cost on representative and pathological collections. | Results record hardware, storage, corpus distribution, settings, cache state, elapsed time, bytes read, CPU use, peak memory, cache growth, hit rate and candidate count. | Should |
| NFR-007 | Visual hierarchy | The system shall allocate at least 80% of work-area height to the main review surface in each reference workspace. | At 1600 × 1000 logical units, measure the grouped table in Findings and the paired region in Compare against their respective work areas. | Must |
| NFR-008 | Portability | The system shall confine operating-system-specific behavior to platform adapter interfaces outside the domain, core, analyzer and storage components. | Path encoding, file identity, hardlink and reflink detection, Trash, file dialogs, external opening and per-user storage locations are reached only through adapters. Platform-conditional compilation appears only in adapter implementations, and the Linux adapters pass the release suite. | Must |

### 7.3 Interface Requirements

| ID | Interface | Requirement | Contract / Format | Acceptance Criteria |
| --- | --- | --- | --- | --- |
| IR-001 | Desktop workspaces | The system shall route review between Findings and Compare while preserving the originating table view. | Two permanent workspaces. The return context carries search, sort, expanded groups, row focus and scroll anchor. | Returning from Compare restores all five view properties. |
| IR-002 | Paired text renderer | The system shall align original source rows across insertions, deletions and unequal wrap heights. | Source line numbers, alignment spacers, linked hunk anchors, named pair score and per-side unmatched content. | Spacers carry no invented line numbers, wrapping preserves original counts and change navigation reaches the same hunk on both sides. |
| IR-003 | Popup forms | The system shall commit scan settings and root selections only on their respective Save or Start actions. | Settings has an editable draft. New scan holds its root draft while Settings is open. Saved preferences apply to subsequent scans. | Cancel or close discards uncommitted edits. Saving leaves the active scan profile intact and returns focus to the invoker. |
| IR-004 | Cleanup adapter | The system shall route source-file mutations exclusively through the file-operation service. | The service accepts a confirmed plan with lossless paths, snapshot references, action, protection and verification preconditions. Analyzers and previews have read-only access. | One confirmation submits one execution. Changed scope requires renewed approval and exact-group bulk cleanup retains a protected, currently verified copy. |
| IR-005 | Analyzer interface | The system shall accept typed analyzer measurements and route their read-only previews through the shared workflow. | Classify, extract, fingerprint, retrieve, compare and describe preview. Results identify analyzer version, snapshot, configuration and completion status. | A fake analyzer with a distinct metric passes through scheduling, persistence, presentation and cleanup planning with shared protection intact. |

### 7.4 Data Requirements

| ID | Data Entity | Requirement | Validation Rules | Ownership |
| --- | --- | --- | --- | --- |
| DR-001 | Scan job and profile | The system shall associate each result set with its scan ID and immutable effective profile. | Record roots, filters, retrieval parameters, thresholds, limits, counters, issues and completion state. Interrupted jobs stay incomplete after restart. | Scan coordinator and local store |
| DR-002 | File entries and snapshots | The system shall retain lossless filesystem paths and versioned observations for operation targets. | Use platform identity where available and record observation time, metadata and stability. A display string or a matching pathname alone cannot authorize deletion. | Discovery and local store |
| DR-003 | Analysis artifacts | The system shall key reusable artifacts by source snapshot or content and relevant analyzer, decoder, profile, configuration and format versions. | Record selected encoding and decoding errors. Invalidate affected artifacts when a dependency changes and expose the basis for reuse. | Analyzer and artifact cache |
| DR-004 | Match relations | The system shall retain typed evidence for the snapshots actually compared. | Record relation type, named metric, direction where relevant, verification level and reference. Distinguish hash candidates, byte verification, normalized equality, resemblance and containment. | Matching engine and local store |
| DR-005 | Operation journal | The system shall persist per-target execution intent before mutation and the observed outcome afterward. | Preserve approval, retained-copy preconditions and recovery evidence. Reconcile uncertain outcomes before retrying and exclude completed items from replay. | File-operation service and local store |
| DR-006 | Local cache and history | The system shall keep rebuildable artifacts separate from source files and the operation history needed for recovery. | Apply cache size and retention controls in per-user storage, omit complete document text by default and retain recovery history across cache clearing. | Cache manager and local store |
| DR-007 | Persistent schema and backups | The system shall preserve consistent local state across supported migrations and backups. | Review schema migrations and use a consistency-aware backup mechanism that includes committed state during active writes. | Local persistence layer |

## 8. Architecture and Design

### 8.1 Architecture Summary

Use a **local, Rust-first desktop application with a media-independent core and a text-specific V1 analyzer**. Keep the GUI, similarity engine, storage, and destructive file operations behind explicit boundaries. Use SQLite for metadata and results, with rebuildable indexes and a bounded disk cache for larger artifacts.

Do not add a server, browser frontend, vector database, AI subsystem, or dynamic plugin platform to V1. Validate the GUI's large-result and comparison workflows early, and benchmark candidate discovery independently from pairwise scoring before freezing the matching design.

Build Fuzzy DeDupe around the specified text-deduplication workflow and a credible path to other media. dupeGuru's feature set and internal structure do not determine the implementation scope. Implement the current compact Figma revision in §11; do not recreate the archived wizard-like workflow or card-based review workspace.

### 8.2 Architecture Views

```text
Desktop application
  Results / read-only comparison / cleanup confirmation
                         |
                         v
Application core
  Scan lifecycle / scheduling / result aggregation / action planning
        |                  |                    |
        v                  v                    v
Filesystem discovery   Analyzer registry    File-operation service
                       Text: planned for V1     Only source-file mutations
                       Image/audio/video:
                       future implementations
        \                  |                    /
         \                 v                   /
          -> Persistence and artifact services <-
             SQLite / versioned fingerprints / bounded local cache
```

The shared platform owns jobs, file identity, persistence boundaries, user selections, and cleanup. Analyzers own media-specific extraction, fingerprints, candidate indexes, metrics, and evidence. The GUI renders that evidence through type-specific comparison views.

The exact-byte service is universal. Its reuse across formats does not imply that the text similarity index can also compare images or audio.

#### Analyzer lifecycle and contract

| Stage | Responsibility | Output |
| --- | --- | --- |
| Classify | Determine supported input and analysis profile. | Media classification and support decision. |
| Extract | Decode useful content with explicit resource bounds. | Media-specific representation and metadata. |
| Fingerprint | Produce stable reusable features. | Versioned fingerprint artifacts. |
| Retrieve | Find plausible candidate relationships. | Candidate pairs, reason, and approximation status. |
| Compare | Score and verify selected candidates. | Typed measurements and evidence. |
| Describe preview | Supply data needed by a read-only reviewer. | Text spans, image references, or future time ranges. |

Every analysis result should identify the source-file snapshot, analyzer ID/version, configuration digest, fingerprint/index format version, and completion status. Stable algorithms with fixed seeds and stable ordering should reproduce results for the same inputs and configuration.

Use a compile-time analyzer registry in V1. “Extensible” means well-separated internal implementations, not a public ABI or runtime plugin loader. Adding a future analyzer may require a new renderer and packaging work; it should not require redesigning shared file safety.

### 8.3 Design Decisions

The owner selected a new Rust desktop application and GPL-3.0-only. GUI framework, database, candidate retrieval, and filesystem adapters are provisional. Each candidate below needs the stated validation before adoption.

#### Foundation

| Layer | Recommendation | Role and rationale |
| --- | --- | --- |
| Language/build | Rust, Cargo, pinned toolchain and committed lockfile | One primary implementation language and reproducible dependency resolution. |
| GUI | egui + eframe | Selected by [ADR 0001](../../adr/adr-0001-use-egui-for-the-v1-desktop-ui.md); final once the MS-1 prototype passes. Keep GUI-specific types out of the engine. [S03] |
| Result tables | egui_extras | Scrolling, resizable table support; application-level virtualization still needs validation. [S04] |
| Persistence | SQLite + rusqlite | Local metadata, scan state, results, and operation records. [S19] |
| Migrations | refinery or a small reviewed SQL migration layer | Versioned schema changes; refinery supports rusqlite. [S20] |
| Exact hashing | blake3 | Full-content cryptographic fingerprints, separate from fuzzy fingerprints. [S10] |
| CPU work | rayon | Parallel analysis with explicit pool limits. [S17] |
| Worker communication | crossbeam-channel | Bounded queues, completion events, and backpressure. [S18] |
| Diagnostics | tracing | Structured scan, comparison, and operation diagnostics. [S24] |
| Configuration | serde + toml | Typed, versioned settings and scan profiles. [S25], [S26] |

This is Rust-first, not a promise of a dependency-free or entirely Rust runtime. Bundled SQLite is compiled from C, and desktop integration still has platform dependencies. `rusqlite` documents its bundled build option. [S19]

Do not introduce an application-wide Tokio runtime without a demonstrated asynchronous workload. A future decoder supervisor or GUI integration may justify async internally; the design does not prohibit it. The V1 engine should not require an HTTP server, FastAPI, Electron, Python runtime, containers, or a browser frontend.

#### Supporting dependencies

| Responsibility | Candidate | Boundary or caution |
| --- | --- | --- |
| Directory traversal | ignore | Configure hidden-file and `.gitignore` behavior deliberately. [S09] |
| Text decoding | encoding_rs | Encoding selection and error policy remain application responsibilities. [S12] |
| Unicode normalization | unicode-normalization | Explicit, versioned normalization profile. [S13] |
| Word/grapheme boundaries | unicode-segmentation | Evaluate tokenization across supported languages and file types. [S14] |
| Fast shingle fingerprints | xxhash-rust | Candidate discovery only; not proof of file equality. [S11] |
| String metrics | rapidfuzz | Optional precise comparison for suitable candidates or spans. [S15] |
| Read-only differences | similar | Line/sequence differences; do not enable merging as a product feature. [S16] |
| Folder/file dialogs | rfd | Validate the selected Linux GTK/portal backend and desktop behavior. [S22] |
| Recoverable deletion | trash, behind a platform adapter | Provisional dependency pending backend and safety validation. [S23] |

Keep the dependency set lean. MinHash/LSH and passage-index integration should live behind an internal interface. Use a suitable reviewed library where it meets correctness, maintenance, and GPLv3 compatibility requirements; otherwise implement the necessary algorithms with reference tests. Writing the application from scratch does not require reimplementing every dependency. It does exclude using dupeGuru as the application codebase or mechanically translating its source into Rust.

#### GUI alternatives and decision gate

| Option | Why it was considered | Recommendation |
| --- | --- | --- |
| egui/eframe | Rust interface, flexible custom views, table support. | **Selected** (ADR 0001); final once the MS-1 prototype passes. [S03], [S04] |
| Slint | Declarative UI with Rust integration. | Designated fallback if egui fails the MS-1 table or paired-pane criteria (ADR 0001). [S05] |
| Iced | Rust GUI with an Elm-inspired architecture. | Rejected (ADR 0001): no built-in table or selectable text; experimental. [S06] |
| Tauri 2 + Svelte | Web UI ecosystem with a Rust backend. | Rejected (ADR 0001): adds a web frontend stack and a dependency on the distribution's WebKitGTK. [S07] |
| Qt + CXX-Qt | Qt/Rust interoperability and another desktop architecture option. | Rejected (ADR 0001): adds a C++/QML toolchain and a frequently changing bridge. [S08] |

The egui project explicitly does not target native-looking widgets and warns of breaking changes. Pin and test compatible GUI dependencies rather than upgrading automatically. [S03]

The GUI decision gate should implement the approved §11 compact layout and demonstrate large result navigation, selection correctness, a usable read-only paired diff, popup behavior, responsiveness during scanning, and an image-preview integration seam. The approved Figma design resolves the visual direction; it does not by itself validate egui or any other runtime framework. Full synchronized video playback does not need implementation in V1 and is not supplied automatically by choosing egui.

### 8.5 Design Constraints

Domain models must not depend on egui or on a specific operating system. Analyzers must not delete, rename, or edit source files. UI code must not implement similarity algorithms or write arbitrary database queries. File-operation decisions must pass through the application service rather than directly from a table callback to a filesystem call.

Isolate operating-system behavior behind platform adapters (C-014, NFR-008). Linux supplies the only V1 implementations, but adapter interfaces must not encode Linux-only assumptions, such as inode-based identity, case-sensitive or byte-string-only paths, or a single Trash location, in their signatures. Prefer dependencies that also support macOS and Windows when they otherwise satisfy V1 requirements.

The core may own trait-based access to storage, analysis, and file operations. Keep orchestration above those components to avoid cyclic dependencies. Avoid embedding SQL connections or GUI widgets in persisted match evidence.

A recommended starting layout is:

```text
fuzzy-dedupe/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── LICENSE               # Full, unmodified GNU GPL version 3 text
├── THIRD-PARTY-NOTICES.md # Applicable dependency and bundled-asset notices
├── DESIGN.md             # Current compact visual tokens and Figma authority
├── UX-CONTRACT.md        # Shared workspace, popup, queue, and safety behavior
├── crates/
│   ├── domain/           # IDs, snapshots, profiles, metrics, events, plans
│   ├── core/             # Use cases, orchestration, registry, scheduling
│   ├── scanner/          # Discovery and filesystem observations
│   ├── analyzers/        # Text implementation and internal analyzer contracts
│   ├── storage/          # SQLite repositories, migrations, artifact cache
│   ├── fileops/          # Revalidation, Trash/deletion, operation reconciliation
│   └── app/              # egui application and presentation adapters
├── tests/
│   ├── fixtures/         # Labeled documents and filesystem edge cases
│   └── integration/
├── benches/              # End-to-end corpus runner and benchmark documentation
├── docs/
│   ├── architecture/
│   ├── decisions/
│   ├── design/           # Figma node manifest, UI acceptance fixtures
│   └── testing/
└── packaging/
```

These are logical boundaries, not a requirement to create seven empty crates on day one. Some can begin as modules and split when ownership and dependencies justify it. Cargo benchmark targets can live in the appropriate package with a shared corpus runner.

The proposed `crates/app/src` UI subdivision and state owners are specified in §11.8; they refine, rather than replace, these architecture boundaries. Keep current Figma links/node IDs in `docs/design/figma-manifest.json`, and map compact tokens centrally through `ui/theme.rs`.

Provide a GUI-independent test/benchmark harness capable of running a profile and exporting measurements. A supported public CLI is not required for V1. Keep application and database schema versions distinct from analyzer and fingerprint versions.

Initialize a new repository for Fuzzy DeDupe and implement the requirements independently. Do not import dupeGuru's repository history, translate its source, or copy its tests or bundled assets as the application foundation. Create project-specific tests and fixtures; record the origins and license terms of third-party dependencies or assets actually included. Keep package metadata and original-source notices consistent with `GPL-3.0-only`, as specified in §18.3.

### 8.7 Concurrency and resource management

#### Resource classes

| Work class | Limit to manage |
| --- | --- |
| Filesystem discovery | Directory operations, open handles, and slow/mounted-volume pressure. |
| Reading/hashing | Concurrent reads per source volume and total buffer memory. |
| Text analysis | CPU workers, decoded-text size, temporary token/shingle storage. |
| Candidate indexing/scoring | Index memory, bucket fan-out, pair count, and task duration. |
| Database writes | One coordinated writer and bounded update queues. |
| Preview generation | Small independent budget favoring interactive requests. |
| Future image decoding | Decoder concurrency, decoded pixels, and memory. |
| Future audio/video analysis | Process count, codec threads, buffers, duration, and optional accelerator resources. |

Rayon provides configurable thread pools; bounded Crossbeam channels provide backpressure between stages. These are building blocks, not a complete scheduler. [S17], [S18]

Start with a small explicit scheduler and resource budgets, not a general distributed job framework. The abstraction should anticipate decoder limits without implementing a video subsystem in V1.

#### Responsiveness and cancellation

The UI must never wait for a blocking scan, decode, database write, or large diff. Workers publish bounded/coalesced progress events; control and cancellation messages must not be trapped behind a full stream of progress notifications.

Place cancellation checks between bounded work units. Do not promise instantaneous cancellation of an uninterruptible filesystem call. Report what completed, what remains, and whether any file operation is already in progress.

Prefer interactive preview work over speculative background comparisons when resources are constrained. Limit nested parallelism: file-level hashing, analyzer jobs, and future decoder-internal threads must not multiply into uncontrolled CPU oversubscription.

#### Honest scalability expectations

Rust, SQLite, and Rayon are appropriate foundations to evaluate, but they do not guarantee unlimited scale. The practical limits will depend on document sizes, candidate density, cache design, storage bandwidth, decoder cost, and GUI query behavior.

Measure cold scans and incremental scans separately. Report cache-hit rates, bytes read, peak memory, and candidate counts alongside elapsed time. A fast scan that omits hard cases without disclosure is not a performance success.

### 8.8 Later media analyzers

#### What remains shared

Reuse source discovery, file identity, exact hashing, job control, resource accounting, scan history, user retention choices, result queries, and the file-operation safety service. Keep per-analyzer artifacts and metrics versioned independently.

What does not generalize unchanged is the definition of similarity. Each format needs its own extraction, candidate retrieval, comparisons, evidence, and reviewer. A universal 0–100 score would obscure important differences between text coverage, image hash distance, acoustic match evidence, and temporal video overlap.

#### Future image analyzer

Evaluate `image` for decoding and image transformations and `image_hasher` for perceptual fingerprints and Hamming-distance comparisons. Both expose relevant primitives, but neither alone establishes the application's accuracy across all image transformations. [S30], [S31]

A proposed pipeline is format validation, orientation/color handling, bounded decoding, normalized thumbnail generation, perceptual fingerprints, candidate retrieval, and more detailed visual verification. Keep original dimensions and metadata available for review.

Set explicit policies for animation, multi-page images, cropping, rotation, watermarks, and severe edits. A simple whole-image hash should not be advertised as handling all of them. Add side-by-side zoom and visual difference views as appropriate; do not add image editing.

#### Future audio/music analyzer

Evaluate Symphonia for audio decoding and Chromaprint for acoustic fingerprint generation. Symphonia is a Rust demuxing/tag-reading/audio-decoding library. Chromaprint consumes decoded audio; it does not itself provide a complete arbitrary-file decoder and duplicate-search database. [S32], [S33]

The analyzer will need candidate indexing and comparison/alignment logic in addition to fingerprint extraction. Preserve duration, channel, sample-rate, and encoding information for review. Distinguish the same recording with different encodings from live versions, covers, remasters, and merely similar music.

Use only local processing. Chromaprint's relationship to AcoustID does not require the application to upload files or fingerprints to a web service. The proposed product needs no online song-identification feature. [S33]

#### Future video analyzer

Evaluate FFmpeg/ffprobe as supervised external processes, potentially wrapped by `ffmpeg-sidecar`, which provides an interface around a standalone FFmpeg binary. This reduces direct library ABI coupling but does not remove binary distribution or codec obligations. [S34], [S35]

A proposed pipeline is metadata probing, bounded frame sampling, per-frame perceptual fingerprints, temporal candidate retrieval, sequence alignment, and closer comparison of candidate intervals. Audio evidence can be an additional signal, not a mandatory prerequisite.

Whole-video equivalence and a short clip contained in a long video are different retrieval problems. Duration filters useful for full copies can exclude partial overlaps. Sparse sampling can miss short matching segments; quality claims require representative testing.

Supervise child processes with bounded output, timeouts, cancellation, temporary-storage limits, and per-process codec-thread limits. Prefer explicit argument arrays over shell interpolation. A separate process provides fault separation but is not a security sandbox; OS restrictions and careful handling of media protocols remain separate decisions.

Initial review can use matched frame strips and timestamps. Synchronized playback should be treated as a distinct integration project, not a reason to implement multimedia playback in V1.

#### Existing architectural precedent

Czkawka's repository documents a reusable Rust core, separate frontends, and analysis for images, music, and video; its Krokiet frontend uses Slint. This demonstrates that a Rust multi-format core is practical, not that its code or algorithms automatically meet this product's requirements. [S36]

Use existing applications and published algorithms as research references for future analyzers. Preserve Fuzzy DeDupe's independently implemented application architecture; do not substitute a fork or port of another application for the confirmed from-scratch plan. Reviewed third-party libraries remain eligible subject to compatibility, provenance, and quality checks.

#### Expansion acceptance gate

A new media analyzer should be added without altering the public meaning of text scores or bypassing file-operation checks. It must supply versioned fingerprints, bounded resource behavior, representative accuracy tests, a usable read-only reviewer, and packaging/license documentation.

Do not add future media crates, decoder binaries, GPU dependencies, or empty service frameworks to V1 merely to demonstrate extensibility. The required preparation is in boundaries, schemas, and contracts.

## 9. Data Model

### SQLite as the local system of record

Use SQLite through `rusqlite`, preferably with a pinned bundled build and reviewed migrations. Store the application database on a local disk even when source files reside on SMB, NFS, or other mounted storage. SQLite WAL supports concurrent readers and a writer, but only one writer at a time, and its documented shared-memory design does not support cross-host network-filesystem use. [S19], [S20], [S21]

Use one coordinated writer, batched analysis updates, bounded transactions, indexes for result queries, and explicit busy/error handling. Choose journal durability deliberately: rebuildable fingerprints and a destructive-operation history do not have the same recovery requirements. Use a supported, security-reviewed SQLite build rather than assuming any bundled version is acceptable.

### Proposed logical entities

This is a conceptual data model, not final DDL.

| Entity | Responsibility |
| --- | --- |
| `scan_profiles` | Roots, filters, analyzer settings, thresholds, and resource policy. |
| `scan_jobs` | Lifecycle, effective configuration, coverage, counters, and errors. |
| `file_entries` | Filesystem locations and platform-specific identities where available. |
| `file_snapshots` | A particular observed version: metadata, observation time, stability, and content references. |
| `content_objects` | Full byte-hash identity and verification provenance. |
| `fingerprints` | Snapshot/content association, analyzer, algorithm version, settings digest, artifact reference. |
| `match_relations` | Compared snapshots, relation type, named metrics, direction, and evidence status. |
| `result_groups` / `group_members` | Exact membership or review clusters and their reference rules. |
| `operation_plans` / `operation_items` | Approved targets, retained copies, preconditions, outcomes, and recovery evidence. |
| `artifact_cache` | Managed artifacts, size, validity, access time, and eviction metadata. |

Paths are not immutable identities. Renames, inode reuse, replacement files, removable mounts, and platform differences must be considered. Store filesystem paths losslessly; a display string may be lossy, but it must never become the authoritative path for deletion.

Store common query fields in typed columns and extensible media-specific evidence in versioned payloads or related tables. Avoid both extremes: one rigid text-only schema and one opaque JSON blob containing everything.

### Artifact and cache policy

Keep small fingerprints in SQLite when practical. Put large previews, future thumbnails, or large media signatures in an application-managed disk cache with size limits and deterministic keys. Analyzer-owned candidate indexes must be rebuildable.

Cache identity should account for the source content/snapshot, analyzer and decoder versions where relevant, normalization profile, fingerprint algorithm, configuration, and format version. Changing one must invalidate the dependent artifacts—not necessarily unrelated analyzers.

File size and timestamps are fast reuse hints, not proof that contents are unchanged. Incremental results should expose their freshness basis. Revalidate content and identity before destructive action. Cache expiration and scan freshness are different concepts.

Do not persist complete document text by default. Reopen original files for review where practical. Fingerprints, paths, and snippets can still disclose information, so a cache is not automatically private merely because it does not contain whole files.

## 10. Behavior and Workflows

### 10.1 Primary Workflow

Choose roots and filters in New scan, run discovery, inspect grouped results in Findings, compare original text, and approve a concrete cleanup plan. A scan can finish with partial coverage. The review must show which files and relationships were actually examined.

#### Keep exact and fuzzy paths separate

```text
Discover eligible text files and record source snapshots
    |
    +-- Exact path
    |     Size grouping -> complete BLAKE3 hashes -> byte verification
    |     -> verified exact groups
    |
    +-- Text path
          Decode -> normalize under a named profile -> tokenize
          -> normalized equality / document sketches / passage fingerprints
          -> candidate retrieval -> pairwise scoring -> match evidence
```

Size grouping is a useful filter for exact duplicates; it must not prevent a small document from being compared with a larger containing document in the fuzzy path. Identical contents may share analysis artifacts, but every filesystem entry must remain individually visible and actionable.

#### Exact duplicate detection

Use complete-file BLAKE3 hashes to identify candidates efficiently. The recommended safety baseline then compares candidate bytes before labeling a group **byte-verified**. Store the distinction between hash-matched and byte-verified evidence. BLAKE3 supports incremental hashing; its optional parallel APIs should be coordinated with application concurrency limits. [S10]

Do not substitute a partial hash, file size, filename, or fast shingle hash for byte verification. Partial sampling can eliminate obvious nonmatches but cannot establish equality.

Hashing can stream through bounded buffers. If metadata or identity changes during a read, mark the observation unstable and retry within a limit or exclude it with an explanation. Treat hardlinked paths separately from independent copies when reporting storage implications.

#### Decoding and normalization

Keep the original file untouched. Decoding and normalization create analysis representations only. Record the profile used for every fingerprint and result.

| Transformation | Recommended handling |
| --- | --- |
| Line endings | Configurable normalization for text comparison. |
| Unicode normalization | Use a named form; NFC is a conservative starting candidate. |
| Leading/trailing or repeated whitespace | Optional profile choice, not universally insignificant. |
| Case | Preserve by default or expose a clear case-insensitive profile. |
| Punctuation | Preserve by default; removal must be deliberate. |
| Markdown/source/config syntax | Analyze literally unless an approved profile specifies otherwise. |
| Decoding replacements/errors | Disclose and exclude from normalized-equality claims unless explicitly handled. |

The proposed Unicode crates expose normalization and segmentation primitives; the application still needs its own profile and language-coverage policy. [S13], [S14]

Handle empty files, whitespace-only documents, files shorter than one shingle, repeated-token documents, and very long lines explicitly. Empty fingerprint sets must not generate meaningless “100% similar” results. Very small documents may need a direct-comparison fallback with its own limits.

#### Document-level resemblance

A shingle is a sequence of adjacent tokens or characters. Representing documents through shingles allows overlapping content to survive many small edits and paragraph movements, while retaining more local structure than an unordered bag of individual words. This is the proposed representation to benchmark, not a guarantee for every language or document type.

MinHash estimates set resemblance; MinHash with locality-sensitive hashing can retrieve likely matches without directly comparing every document pair. The method is approximate and can produce false positives and false negatives. [S27], [S28]

Use fixed seeds, stable serialization, and deterministic tie-breaking. This makes runs reproducible without claiming that an approximate index has exhaustive recall. Compare sketches with exact shingle-set measurements on benchmark samples and on retrieved candidates where resource limits permit.

Select shingle length, sketch size, banding, and thresholds from measured accuracy/cost tradeoffs. Do not freeze arbitrary defaults simply because an example implementation uses them.

#### Passage overlap and containment

Whole-document resemblance is not sufficient for a small document contained within a much larger one. Add a separately evaluated passage-oriented retrieval path, such as local fingerprints with an inverted index or a containment-aware index.

Winnowing is one research candidate for local document fingerprinting and partial-copy detection. Its author's page includes implementation errata and a historical patent discussion. Consult the errata when evaluating it; the historical notice alone does not establish a current legal restriction on this project. [S29]

The passage path should preserve locations so the comparison view can show actual shared spans. A matching fingerprint only proposes a match; verify the underlying spans before presenting them as confirmed matching text.

Shared boilerplate needs special treatment. Evaluate minimum matched-span length, minimum coverage, and corpus-frequency filtering so a common license header does not overwhelm results. Any suppression that can hide valid matches must be measurable and disclosed through profile settings or diagnostics.

#### Use separate, defined measurements

For distinct shingle sets `A` and `B`, define:

```text
intersection = |A intersect B|
union        = |A union B|

resemblance(A, B) = intersection / union
coverage_of_A    = intersection / |A|
coverage_of_B    = intersection / |B|
smaller_set_containment = intersection / min(|A|, |B|)
```

The first is Jaccard resemblance. The two coverage measures show direction explicitly. Smaller-set containment is a convenience summary, not a substitute for showing which document is covered. Define empty-set behavior separately rather than dividing by zero. MinHash estimates Jaccard; it does not automatically establish passage-level coverage. [S27]

For example, if `A` has 2,000 distinct shingles and `B` has 100, all contained in `A`, resemblance is 5%, coverage of `A` is 5%, and coverage of `B` is 100%. Those are exact values for the hypothetical sets—not percentages of words, semantic equivalence, or a deletion recommendation.

Set metrics also discard repetition counts. Keep sequence-aware differences and verified spans available so repeated or rearranged material is not concealed by a single set score.

#### Precise comparison and visual differences

Use `similar` for line/sequence differences and consider `rapidfuzz` for selected edit-distance or string metrics. Neither should be assumed to provide scalable collection-wide candidate discovery by itself. Their documented functions support comparison of supplied inputs. [S15], [S16]

Generate expensive diffs on demand, cache a bounded number, and cancel obsolete requests when the user changes selection. Word-level highlighting can refine line-level changes. A moved section may appear as deletion plus insertion unless a separate move-detection pass is implemented; do not promise that behavior merely by adding a diff crate.

#### Candidate coverage and threshold changes

A naive all-pairs scan requires `n * (n - 1) / 2` comparisons: 100,000 documents produce 4,999,950,000 pairs. Candidate retrieval is therefore a fundamental component, not a cosmetic optimization.

The engine must record retrieval parameters separately from the final score/display threshold. Lowering a display threshold cannot reveal candidates that the index never retrieved. When necessary, changing sensitivity must trigger requerying or rebuilding the relevant index rather than silently filtering an incomplete old result set. Approximate LSH retrieval specifically has recall tradeoffs. [S28]

Bound pathological buckets, queues, and intermediate results. A budget limit may yield incomplete coverage; it must not silently discard candidates while reporting a complete scan. Candidate retrieval, pairwise scoring, and GUI ranking need separate benchmarks.

#### Grouping policy

Exact byte equality supports equivalence groups. Store membership without materializing every possible pair within a large exact group.

Fuzzy similarity does not imply transitivity. If A matches B and B matches C, A may not match C. Recommended presentation is a reference-centered group or an explicitly labeled connected cluster that retains its pairwise evidence. The UI must identify whether a score is relative to the selected reference, another file, or a cluster summary.

Changing the reference file must update displayed scores. Files connected only through an intermediate match must not receive an invented direct score. Similarity grouping must never grant automatic deletion authority.

Decoding must record the selected encoding and whether errors occurred. Prefer valid UTF-8, then recognized byte-order marks or an explicit encoding choice. Heuristic encoding detection, if added, must be disclosed and separately evaluated. `encoding_rs` provides decoding, not a guarantee that an arbitrary file's encoding can be inferred correctly. [S12]

“Keep newest,” “keep largest,” or preferred-folder rules may assist exact-duplicate selection. They must not be described as discovering the most complete or correct version of a fuzzy match.

### 10.2 Alternate Workflows

Opening a file externally returns control to its native application. A rescan checks freshness before cached evidence can inform cleanup.

### 10.3 Edge Cases

#### Hardlinks, reflinks, and disk-space reporting

Different paths may refer to the same underlying file or share storage. Btrfs documents shared extents between reflinked files and snapshots, so apparent duplicate file size is not a reliable promise of space reclaimed by deletion. [S37]

Report logical duplicate bytes separately from estimated reclaimable space. Label unknown physical savings honestly. Do not offer automatic hardlink/reflink replacement in V1; it introduces different correctness and filesystem semantics from removing an unwanted copy.

### 10.4 State Transitions

Only the file-operation service may modify source-file locations. Analyzers and previews are read-only. The service accepts an explicit, user-approved plan containing lossless paths, source snapshot references, desired actions, protected files, and verification preconditions.

Recommended action states are `planned`, `approved`, `validating`, `executing`, `succeeded`, `failed`, `skipped`, and `needs_reconciliation`. The exact state machine should be tested before connecting live deletion to the GUI.

## 11. UI Pages / API Endpoints

### 11.1 Authoritative design links and revision precedence

**Implement the compact desktop revision, not the archived multi-screen exploration.** The owner approved the streamlined direction and the subsequent Figma update. Its defining requirements are a table-dominant findings workspace, large side-by-side comparison panes, compact controls, fewer main screens, and popup windows for settings.

**Primary entry points:** [Open the approved Figma design][UX01] · [Open the interactive prototype][UX02] · [Open the comparison workspace][UX03].

| Design reference | Figma node ID | Implementation destination |
| --- | --- | --- |
| [Current page: 01 · Compact desktop][UX04] | `13:529` | Current design reference; all new UI implementation starts here. |
| [01 · Findings — compact table][UX01] | `13:648` | `FindingsView`: primary post-scan workspace. |
| [02 · Compare — large paired panes][UX03] | `13:1304` | `CompareView`: primary read-only inspection workspace. |
| [Popup · Settings][UX05] | `16:735` | `SettingsDialog`: matching rules and normalization. |
| [Popup · New scan][UX06] | `16:806` | `NewScanDialog`: folders, protection, and scan initiation. |
| [Popup · Review cleanup][UX07] | `16:886` | `CleanupDialog`: plan, final authorization, progress, and outcomes. |
| [Popup · Review unique text][UX08] | `16:932` | `UniqueTextReviewDialog`: deliberate acknowledgment for a specific candidate. |
| [Components · Compact desktop][UX09] | `13:538` | Shared compact controls, grouped rows, and queue indicators. |
| [Comparison prototype entry][UX10] | Starting node `13:1304` | Direct entry for checking the large paired-pane interaction. |

The file key is `d5tA6ap1tH4cTDaJiwve89`. API node identifiers use colons; Figma URL parameters use hyphens. All node IDs and the measurements below were inspected in the live file on **November 11, 2026**. Links require access to the Figma file. This Markdown is self-contained for the implementation requirements; Figma supplies the visual reference.

The earlier page, **Archive · Initial exploration** (`0:1`), and its group-card workbench, dedicated configuration screens, separate cleanup/outcome pages, and compact unified-diff example are historical material. They are **not** the current layout specification. Earlier downloadable design documents and research recommendations must not override this revision.

Precedence is: explicit owner decisions and safety requirements → this current compact-UI contract → current Figma frames/components → older research and archived examples. Geometry is a reference target, not a reason to break text or display scaling. Do not infer new business rules from decorative elements or sample data.

Use **Fuzzy DeDupe** as the implemented application's visible product name. The linked Figma file, frame names, and component identifiers may retain earlier generic naming; preserve their node IDs and use the confirmed product name in application titles and project documentation.

**Prototype limitation:** workspace navigation and popup opening/closing are demonstrated. Actual filters, scans, queue persistence, settings validation, linked scrolling, and file operations are not implemented by those links. A frame or layer named “virtualized” is a design intention, not evidence of a working virtualized table. Sample filenames, counts, scores, thresholds, and queue contents are fixtures—not production defaults or algorithm outputs.

### 11.2 Two workspaces; supporting tasks use popups

Use one application shell with **two persistent workspaces: Findings and Compare**. The narrow navigation rail contains four entries, but only two are workspace destinations:

| Entry/action | Runtime behavior | Context retained |
| --- | --- | --- |
| New scan | Open the folder/configuration popup above the active workspace. | Current scan, findings, and queued decisions remain available until a deliberate replacement. |
| Findings | Show the full grouped findings table. | Filters, sort, expanded groups, focused row, widths, and scroll anchor. |
| Compare | Show the last valid pair, or remain disabled with an explanation when no pair exists. | Pair identity, original/normalized mode, split ratio, and change position. |
| Cleanup · N | Open cleanup review for the current queue. | Return to the invoking workspace; do not add another permanent screen. |
| Settings gear | Open settings above the active workspace. | Draft edits are separate from saved settings and existing scan evidence. |

Scanning belongs to the existing shell: use a compact progress/status area and an optional details popup. Cleanup progress and per-file outcomes belong in the cleanup dialog, with an expandable journal/details view. Do not rebuild the previous scan-progress and operation-outcome pages merely because they exist in the archive.

The primary flow is:

```text
Findings workspace (empty before the first scan)
  -> New scan popup -> start background scan
  -> Findings table with completed/partial coverage status
  -> Compare selected file against the named reference
  -> Keep both, or deliberately queue a specific file
  -> Cleanup popup: review/revalidate/confirm
  -> Outcomes in the same popup -> return to the prior workspace
```

Compactness must come from removing repeated navigation, headings, cards, and unnecessary screens—not from hiding file identity or bypassing cleanup safeguards.

### 11.3 Layout measurements and visual hierarchy

The approved frames are **1600 × 1000 design units**. Map the geometry to logical UI units and scale with display/text settings; do not hardcode physical monitor pixels or canvas coordinates. The source frames contain incidental one-unit border offsets; do not reproduce an overflow bug just to copy those offsets.

| Element | Inspected reference geometry | Runtime rule |
| --- | --- | --- |
| Application title/chrome | 40 high | Compact title. Use appropriate OS decoration; do not draw a duplicate title bar beneath native chrome. |
| Status bar | 26 high | One compact status line; expand only for a necessary state, not permanent summary cards. |
| Navigation rail | 152 wide | Narrow, text-labeled navigation; collapse before sacrificing comparison width. |
| Navigation item | 136 × 34 | Consistent hit area and selected state. |
| Work area | 1448 × 934 | Consume remaining window space. |
| Work-area inset | 12 each side | Avoid the former 24–40-unit page padding. |
| Toolbar | 32 high; controls 30 high | Single compact action row; overflow secondary actions at smaller sizes. |
| Filters/evidence/change rows | 28 high | Inline information, not a row of large cards. |
| Vertical gaps / pane separator | 8 | One consistent compact rhythm. |
| Findings table | 1424 × 822 | Dominant findings element; fills remaining work-area height. |
| Table header / body rows | 30 / 28 high | Sticky header and compact, stable-height rows at default text scale. |
| Combined comparison panes | 1424 × 798 | Dominant comparison element; no competing results table above them. |
| Each document pane | 708 wide at the equal split | User-resizable divider; equal widths initially. |
| Pinned document identity header | 54 high | Filename, accessible full path, and side/protection status. |
| Visible text area per pane | 744 high | Text uses available space; headers and controls stay compact. |

At the reference window size, the findings table occupies about **88% of the work area's height**; the comparison region occupies about **85%**. These proportions are useful regression targets. They are not minimum-size guarantees, and should not be enforced by clipping content under text or display scaling.

The layouts should read as:

```text
FINDINGS                                COMPARE
Compact title                           Compact title
Narrow rail | search / filter / actions  Narrow rail | back / pair / actions
            | relationship filters                  | compact pair evidence
            |                                       | change / wrap / scroll
            | LARGE GROUPED TABLE                   | DOCUMENT A | DOCUMENT B
            |                                       |            |
            |                                       | LARGE READ-ONLY PANES
Compact status                          Compact status
```

### 11.4 Findings table: structure and interactions

Use a grouped, virtualized table rather than cards or an always-visible preview sidebar. The selected group expands into member rows; the reference remains identifiable. The table owns vertical scrolling, with the header and column widths aligned. It is the main interaction surface after a scan. [UX01]

| Default column | Reference width | Meaning |
| --- | --: | --- |
| Queue | 56 | Explicit pending cleanup intent; never ordinary row focus. |
| Group | 100 | Stable group label and expand/collapse control. |
| Match type | 122 | Same bytes, Same text, Similar text, or Containment. |
| Resemblance | 110 | Pair-specific measurement relative to the named reference. |
| File name | 352 | Primary file identity; allow resize and full-value inspection. |
| Folder | 362 | Location; shorten only the visual representation. |
| Protected | 96 | Explicit protection/retention restriction, independent of focus. |
| Decision | 210 | Unreviewed, keep/reference, queued, blocked, or reviewed state and actions. |

The column widths total 1408 units; the outer table additionally reserves space for borders/scrolling. Treat file-name and folder widths as flexible. Size, modification time, and additional verification metadata may be optional columns or compact details; do not add permanent panels to show every field.

**Interaction contract:**

- A single row click focuses the file for inspection. Enter, double-click, or the Compare action opens that file against an explicitly named reference. A reference row with no selected counterpart does not compare against itself.
- The queue checkbox and decision menu are separate actions. Checkbox clicks must not accidentally open comparison; expanding a group must not change queued files. Protected references have no actionable removal checkbox.
- Ordinary group-header clicks expand/collapse. Do not interpret a group checkbox or a global selection shortcut as authorization to delete every member. Any bulk-selection aid must specify scope and protect a currently valid retained copy.
- Fuzzy or normalized-text removal intent goes through the same review policy regardless of whether it originated from a checkbox, context menu, shortcut, or comparison button. An unreviewed score never creates approved removal intent.
- Sorting and filtering preserve focus/queue membership by stable IDs, not row index or filename. A filtered-out queued file remains counted: for example, “3 queued; 1 outside filter.”
- Search is filename/path filtering, not an implied full-text search engine. Include a clear control when populated. Distinguish no matching rows from a scan that found no relationships.
- Keep groups intact when sorting; define group sort and within-group sort separately. Retain the reference as context when a filter matches one member; mark context-only rows rather than misrepresenting them as filter matches.
- Show match scores on member relationships. Do not put a universal percentage on a mixed fuzzy group or imply all member pairs passed the same threshold. Define how category counts handle groups containing more than one relationship type; a sum-to-All presentation needs mutually exclusive display categories.

Reference UI labels are **Same bytes**, **Same text**, **Similar text**, and **Containment**. “Same text” means equality under a recorded normalization profile, not original-byte equality. Containment must state direction, such as “98% of B appears in A,” rather than reuse the resemblance percentage without explanation.

The table should expose optional verification detail for hash candidates versus byte-verified duplicates. The displayed label and engine verification state must agree.

### 11.5 Comparison: large, aligned, read-only panes

The comparison workspace replaces the findings table; it does not retain a large table, group-card column, or permanent inspector beside the documents. Keep only the narrow rail, compact toolbar, pair evidence, change navigation, and the two panes. [UX03]

The paired panes share a resizable split and show pinned file identities. At the reference size each pane has 708 units of width, including a line-number gutter and scrollbar. Default document text is IBM Plex Mono, 14-unit size with 22-unit line height. Text must be selectable/copyable without allowing edits. There are no Save, merge arrows, editable cursors, or inline content-modification controls.

**Diff rendering requirements:**

1. Generate diff/evidence off the UI thread. Use the current file snapshots and comparison-profile revision as cache keys; never show stale results as current.
2. Render aligned row pairs. When one side lacks a line, insert an alignment spacer without a fabricated source line number. With wrapping enabled, align each paired row to the greater of its two measured heights.
3. Preserve original source line numbers separately from visual wrap rows. Counts such as “Only in A: 3 lines” refer to a documented original-content measurement, not the number of wrapped screen lines. Identify normalized-mode counts explicitly.
4. Link vertical scrolling by alignment-row/hunk anchors, not merely by applying identical raw pixel offsets to unrelated documents. Let users unlink scrolling for independent inspection.
5. Keep wrap and linked-scroll controls compact and persistent. With wrap off, provide deliberate horizontal scrolling and retain the line-number gutter. Cache text layout by file revision, font scale, wrap width, and view mode.
6. Previous/next change moves both panes to the same change. Provide previous/next candidate access through compact controls or a menu, without reintroducing a persistent group panel.
7. Use color **and** side labels/symbols. The current blue/amber highlights indicate A-only versus B-only content; they do not mean good/bad, older/newer, or an established revision history.
8. Original decoded text is the default evidence. A normalized view, when offered, is explicit and must preserve a path back to original differences.
9. Load only the requested evidence and visible text ranges. For truncated/oversized/unreadable previews, state what could not be displayed; never infer equality from a blank pane.

The metadata line contains match type, a named pairwise score, and unmatched content on both sides. Definitions/profile details belong in a keyboard-reachable tooltip or details popup, not in a permanent large evidence card.

**Keep both** records a no-removal decision for the displayed pair and clears pending removal intent for that pair where applicable; it must not silently change unrelated members of a larger group. **Queue right file…** identifies the actual target by ID and path, and opens the review acknowledgment when required. The comparison reference is an evidence anchor; protection is a separate restriction, even when the example file has both roles.

Returning to Findings restores the exact search, sort, expanded groups, row focus, and scroll anchor that opened the comparison.

### 11.6 Popup contracts and compact progress handling

All application-owned popups use shared dialog behavior: an explicit title, reachable close/cancel controls, visible focus, keyboard dismissal where safe, constrained size, and focus restoration to the invoking control. Modal dialogs block underlying shortcuts as well as pointer interaction. Use a draft model for forms; closing a popup never commits its changes.

| Popup | Reference size | Required behavior |
| --- | --- | --- |
| Settings | 420 × 400 | Match mode, resemblance threshold, normalization, Cancel/Save. Saved rules apply to the **next scan**; existing results retain their immutable profile. |
| New scan | 640 × 430 | Root list, add/remove root, protection, settings entry, Cancel/Start. Root removal here changes scan configuration—not source files. |
| Review cleanup | 760 × 406 | Concrete target table, retained-copy checks, destination, blocking conditions, Cancel, and final “Move N files to Trash.” |
| Review unique text | 480 × 324 | Target identity, unmatched-content consequence, acknowledgment, Cancel/Queue. Queue stays disabled until required acknowledgment is present. |

These are initial dimensions, not fixed bounds for all content. Grow or internally scroll when paths, validation errors, localization, or larger text require more space; keep the action footer reachable.

**Settings implementation:** keep saved preferences, a dialog draft, and the active scan's profile separate. Validate numeric ranges using the selected metric. The displayed `95.0%` is an illustrative value, not an approved algorithm default. Containment needs its own threshold/direction if enabled; the simple popup does not resolve all advanced controls. Exclusions, encoding policy, and resource limits can use progressive disclosure or tabs inside the popup rather than new main screens. Turning an option off must not rewrite old evidence under a different rule.

**Popup nesting:** opening Settings from New scan preserves the root-list draft. Closing Settings returns focus to New scan. Use a small owned-dialog stack or a single dialog host with a return context; avoid unbounded nested native windows.

**Scanning:** Start launches a worker job and returns to the existing shell with progress. Before the first result set, the table region shows a useful empty/progress state. If another scan is already available, retain and clearly label it until the new scan is ready. Do not merge result sets from different scan IDs accidentally. Stop requests cancellation; completion/partial coverage appears only after worker acknowledgment. Keep counts and issue details accessible through compact controls.

**Cleanup:** one dialog hosts preflight, review/confirmation, execution progress, and outcomes. Do not restore a separate review screen followed by another generic “Are you sure?” screen for the ordinary Trash path. Initially focus Cancel for the final destructive action. Disable repeated submission during execution. Report each failure, skipped item, and uncertain result in the same dialog; allow targeted retry only after reconciliation and revalidation. Large plans use a virtualized internal table.

Any change to the confirmed targets, retained references, or required review evidence invalidates authorization. Block affected items and obtain confirmation of the revised plan rather than silently changing the displayed scope. No silent fallback from Trash to permanent deletion is allowed. Permanent deletion, if included, remains a separately designed, explicitly authorized action—not an automatic error-recovery path.

### 11.7 Centralized visual tokens and reusable widgets

The following colors were read from the existing Figma semantic variables. Reuse them through one Rust theme adapter; never scatter independent hex literals through the views. [UX04], [UX09]

| Token | Value | Role |
| --- | --- | --- |
| `canvas` / `surface` / `rail` | `#F4F6F9` / `#FFFFFF` / `#EBEFF5` | Work area, table/document surface, navigation. |
| `ink` / `muted` / `quiet` | `#202C3D` / `#5B687A` / `#637185` | Main, secondary, and compact supporting text. |
| `primary` / `primarySoft` / `primaryHover` | `#325CC9` / `#E9EFFD` / `#2449AA` | Primary actions, focused-row tint, interaction states. |
| `green` / `greenSoft` | `#236C54` / `#E9F4EF` | Protected/verified/completed status, with text. |
| `amber` / `amberSoft` | `#8B5614` / `#FFF2DC` | Unmatched content or recoverable caution, with labels. |
| `red` / `redSoft` | `#AA353B` / `#FCECEE` | Final destructive action and failures. |
| `line` / `code` | `#DDE3EB` / `#FAFBFD` | Separators and quiet document/header backgrounds. |

| Compact text style    | Family / weight        | Size / line height |
| --------------------- | ---------------------- | ------------------ |
| Title                 | IBM Plex Sans SemiBold | 16 / 22            |
| Body                  | IBM Plex Sans Regular  | 14 / 20            |
| Control label         | IBM Plex Sans Medium   | 13 / 18            |
| Table                 | IBM Plex Sans Regular  | 13 / 18            |
| Group/header emphasis | IBM Plex Sans SemiBold | 13 / 18            |
| Supporting text       | IBM Plex Sans Regular  | 12 / 18            |
| Document              | IBM Plex Mono Regular  | 14 / 22            |
| Path/gutter detail    | IBM Plex Mono Regular  | 11 / 16            |

Use the `DD/Compact/*` style family for this revision, not the larger `DD/title` and archived page headings. Shared compact controls use a 5-unit radius in the inspected frames. Routine rows remain rectangular; shadows belong to floating dialogs, not every table section. Keep selected, queued, protected, stale, and disabled states visually and behaviorally distinct.

Preserve usable hit targets despite compact glyphs: the 16-unit checkbox is a visual mark inside an adequately sized cell target, not the entire clickable area. Icons need a visible label or tooltip; important destructive controls retain text labels. Obtain licensed font assets through the project's dependency/asset process and provide appropriate fallback coverage; the handoff does not distribute font files.

Figma's imported Simple Design System components are visual references. They do not require React, a browser runtime, a web component package, or Figma connectivity in the application. Recreate their behavior with the chosen Rust UI stack and centrally owned widgets.

### 11.8 Recommended Rust UI modules and state ownership

The approved layout does not change the Rust/egui architecture selected by ADR 0001. The module names below are proposed implementation boundaries, not claims that source code already exists. [S03], [S04]

```text
crates/app/src/
  app.rs                     # UI entry point; bounded event ingestion
  ui/
    shell.rs                 # Compact chrome, rail, status, workspace switch
    theme.rs                 # Semantic palette, typography, density, scaling
    findings.rs              # Grouped table, filters, sorting, inspection focus
    compare.rs               # Pair routing, controls, text-renderer host
    widgets/
      grouped_table.rs       # Stable row identity + virtualization
      diff_view.rs           # Paired alignment, wrap, selectable read-only text
      dialog_host.rs         # Modal ownership, focus, return context
      status.rs              # Compact progress and per-operation feedback
    dialogs/
      settings.rs
      new_scan.rs
      cleanup.rs
      unique_text_review.rs
  presentation/
    state.rs                 # Workspace/dialog state and restoration
    commands.rs              # Typed requests to application services
    events.rs                # Job/result/operation updates to presentation state
    projections.rs           # Domain results -> rows, metrics, viewer data
```

Reuse the domain/core/storage/fileops boundaries in §8.2 and §8.5. UI modules emit commands; they never hash files, run large diffs, execute SQL directly, or delete paths during widget drawing.

| State owner | Required contents/invariant |
| --- | --- |
| `WorkspaceState` | Exactly Findings or Compare; dialog visibility is independent. |
| `FindingsState` | Scan ID/revision, filter, sort, expanded group IDs, focused row ID, column widths, and scroll anchor. |
| `CompareState` | Reference/candidate IDs and snapshots, evidence revision, mode, wrap, linked scroll, split ratio, active change, return context. |
| `DialogState` | Popup kind, invoking control, draft values, validation, pending state, and bounded parent return context. |
| `QueueState` | Stable target IDs, intended action, retained-reference IDs, review/protection state, and evidence-bound acknowledgment. |
| `ScanState` | Immutable effective profile, generation/job ID, stage, counts, errors, and completion/partial status. |
| `OperationState` | Engine-issued plan revision, authorization, per-file validation/execution outcomes, and journal reference. |

Keep these concepts separate: focused row; files selected for an explicitly scoped batch action; queued cleanup intent; and engine-authorized immutable operation plan. Changing one must not implicitly grant another. A path string rendered in the UI is never the authority for destructive action. Queue membership is canonical per underlying target identity: the same file appearing in multiple relationship groups cannot produce duplicate removal operations. All affected rows reflect a protection or queue change consistently.

Recommended commands include `StartScan`, `CancelScan`, `LoadComparison`, `RequestQueue`, `AcknowledgeUniqueText`, `RemoveFromQueue`, `ProtectFile`, `BuildCleanupPlan`, and `ExecuteApprovedPlan`. Responses carry the originating scan/job/request revision so stale responses cannot replace the active view. Do not use an unrestricted `DeletePath(String)` callback from a table row.

Persist useful local presentation preferences, but do not restore deletion authorization blindly after restart. Recovered queue entries require current identity/evidence checks. Evidence acknowledgments bind to the specific target/reference snapshots and profile; changing either invalidates them.

### 11.9 Rendering, asynchronous work, and scaling

For the egui implementation (ADR 0001), compose the shell from bounded regions and keep the table or paired viewer as the flexible remainder. Prefer the existing table/scrolling facilities where they meet the requirements; exact API calls must be checked against the pinned crate versions during implementation. The architecture here does not prescribe an unverified code generator or a browser frontend. [S03], [S04]

Flatten expanded group headers/member rows into a visible-row projection with stable row keys. Render only the visible range plus a small overscan margin. Rebuild projections on data/filter/sort/expansion changes—not on every frame. Keep loading, empty, error, and populated table states within the same geometry. If paging is used internally, preserve group context across page boundaries.

Diff decoding, matching, and expensive text layout run through bounded worker/cache services. The UI consumes bounded event batches and coalesced progress; it does not wait for file I/O. Cancel or ignore superseded comparison requests. Keep interactive comparison work responsive while a scan runs, subject to the shared resource scheduler in §8.7.

A common aligned-row model is preferable to two unrelated text widgets with loosely synchronized scrollbars. Cache row heights for the current wrap widths/font scale. Do not instantiate a complete editable multiline control for each entire large file merely to obtain text selection. Validate read-only selection, scrolling, and keyboard operation on representative files before freezing the widget implementation.

Recommended frame-level measurements include event-queue size, rows rendered, comparison-load latency, layout-cache reuse, and cancellation responsiveness. Performance targets require measurement; a Figma frame is not evidence that a particular corpus size is supported.

### 11.10 Resizing, scaling, and keyboard behavior

The current approved comparison default stays **side by side**. Do not silently adopt the old 1280 × 800 unified-diff design. At smaller window sizes, collapse the rail, move secondary controls into menus, and reduce optional table columns before reducing document readability. A user-selected unified view can be considered separately; it is not a replacement for the approved paired view.

At increased text scale, grow rows and controls as needed, constrain dialogs to the usable viewport, and permit body scrolling with fixed action footers. The 28-unit table row and 30-unit control are default-density references, not immutable limits under scaling. Preserve full-path access through keyboard-reachable details/copy actions; hover alone is insufficient.

Proposed keyboard mappings must follow native platform conventions and avoid text-entry/IME conflicts: Tab traverses regions/controls; arrows navigate the table; Enter opens comparison; Space acts on the focused queue control subject to protection/review rules; Escape closes a popup or returns from comparison without committing. No single-key unconfirmed filesystem deletion is allowed. Opening/closing dialogs restores focus and must not accidentally dispatch a command to the underlying table.

Keep row focus, checked queue state, and protection visually and behaviorally distinct. Show status for scans, selection counts, blocked operations, and results in the compact status area or the owning popup. Assistive-technology support, such as screen-reader names and announcements, is a non-goal (NG-003); keyboard operation and display scaling remain required (NFR-005).

### 11.11 Acceptance checklist for the implementing agent

The following are implementation acceptance requirements derived from the approved layout and the safety baseline. The exact test harness and numerical performance budgets remain engineering decisions.

| ID | Acceptance condition |
| --- | --- |
| UI check 01 | Only Findings and Compare are permanent workspaces; New scan, Settings, and Cleanup open popups without losing workspace state. |
| UI check 02 | At the reference size, the findings table and comparison region each occupy at least 80% of their respective work-area height; no competing card dashboard or persistent inspector is introduced. |
| UI check 03 | Compact row/control sizing and `DD/Compact/*` typography match the reference while scaling with text and display settings. |
| UI check 04 | Sorting, filtering, collapsing groups, and returning from comparison do not change queued membership or the identity of a targeted file. |
| UI check 05 | The table contains defined relation/score semantics; focus and cleanup intent are independently visible. |
| UI check 06 | Paired diffs remain aligned through inserted/deleted lines and different wrapping heights; source line numbers and unique-text counts remain correct. |
| UI check 07 | Original text is inspectable, comparison is read-only, and no editor/merge/AI controls are present. |
| UI check 08 | Cancel/close leaves drafts uncommitted; saved settings affect subsequent scans without relabeling current evidence. |
| UI check 09 | All queue entry points enforce the same protected/stale/fuzzy-review policy. A changed target or retained copy invalidates the affected approval. |
| UI check 10 | Cleanup review states exact targets/destination; commit is single-flight; Trash failure cannot become permanent deletion. |
| UI check 11 | Partial, failed, skipped, and uncertain outcomes remain visible in the same popup and operation journal; retry does not replay completed actions. |
| UI check 12 | Large-result table scrolling and large-file comparisons remain responsive using bounded background work and rendering. |
| UI check 13 | Keyboard-only operation, focus restoration, long paths, Unicode, and display scaling are tested on supported platforms. |
| UI check 14 | Missing roots, empty scans, no filter matches, decoding errors, stale evidence, scan cancellation, and unavailable Trash have actionable states without extra permanent screens. |
| UI check 15 | Future media can replace the comparison renderer/metrics without changing the shared findings, queue, protection, and operation model. |

The Figma prototype satisfies none of the runtime safety/performance tests by itself. An agent must implement and verify the behaviors, not merely make its screenshots resemble the design.

### 11.12 Implementation order for this UI revision

First build the compact shell, grouped-table projection, paired-view layout, and shared popup host against deterministic fixture data. Match the linked frames before adding elaborate controls. Then implement navigation/state restoration and the queue/review state machine with tests. Connect real scanning/comparison services next; enable actual Trash operations only after the §12 safety gate.

Test the two dominant workflows independently: efficient table review across many findings, and prolonged reading of large side-by-side documents. Do not optimize one by permanently shrinking the other. Retain the approved visual arrangement while validating the still-provisional GUI framework, matching defaults, format boundary, and platform support.

## 12. Error Handling and Recovery

### 12.1 Expected Failures

| Situation | Required baseline behavior |
| --- | --- |
| Exact group bulk selection | Preserve a protected, currently verified copy; reject plans that remove all copies. |
| Fuzzy/normalized match | Require deliberate selection and review; no score-driven automatic deletion. |
| File changes after scan | Invalidate the approval or return the item for re-analysis. |
| Trash unavailable | Fail safely or offer a separately confirmed action; never silently switch to permanent deletion. |
| Filename/path replacement | Do not operate on an unverified replacement merely because its path matches. |
| Partial batch failure | Continue or stop according to policy, but report each outcome accurately. |
| Optional copy/move | Never overwrite silently; cross-filesystem operations need copy/verify/remove semantics and partial-failure handling. |
| Unknown crash outcome | Mark for reconciliation rather than assuming success or retrying destructively. |

Permanent deletion is not “secure erase” and should not be marketed as such. Trash is a recovery aid, not a guarantee of an in-application undo. Guaranteed restoration, undelete, and filesystem snapshots are not part of the V1 promise.

| ID | Failure | Required response |
| --- | --- | --- |
| ERR-001 | The operation target or retained copy no longer matches the approved snapshot. | Invalidate approval and return the affected item for review. |
| ERR-002 | The filesystem action and journal disagree after interruption. | Preserve the uncertain outcome and reconcile the item before retry. |

### 12.2 Retry and Idempotency

For each approved target, verify identity and freshness, confirm the selected action remains valid, persist execution intent, perform the filesystem operation, and record its observed outcome. Changes invalidate the affected results and preview artifacts.

A database transaction cannot make an arbitrary filesystem operation atomic with the journal. A crash between the filesystem action and recording completion creates uncertainty. Reconcile using the original path, recorded identity, destination/Trash evidence, and current state; do not blindly replay an ambiguous delete against whatever now occupies the path.

Revalidation reduces stale-result risk but does not eliminate every time-of-check/time-of-use race. Use platform-appropriate handle and identity checks where possible, refuse ambiguous cases, and document residual limits on concurrently changing or adversarial directories. Do not claim race-free deletion without proving the platform implementation.

### 12.3 Rollback / Recovery

An interrupted scan may retain completed artifacts, but its overall status remains interrupted or incomplete. On restart, reconcile unfinished operation records before allowing related targets to be retried.

Provide a safe cache-rebuild path. Cache deletion must not remove source files or the only operation history needed for recovery. Database backup/export must use a consistency-aware mechanism rather than copying only the main file during active WAL writes. [S21]

As checked on November 11, 2026, the `trash` documentation still describes a potential Linux/FreeBSD undefined-behavior issue involving non-thread-safe mount-information APIs. It documents an internal mutex but cautions about other code invoking those APIs. It also describes alternative portal behavior and limitations. Therefore, `trash` is a **provisional** dependency, not an unqualified safety endorsement. [S23]

Evaluate the chosen backend in the complete application process, including dialogs and other dependencies. Isolating deletion in one application thread is not by itself proof that unrelated calls cannot conflict. The file-operation adapter must allow a reviewed alternative backend or implementation.

## 13. Security and Privacy

### 13.1 Authentication

The proposed application runs under the local user account and requires no service login.

### 13.2 Authorization

Only the file-operation service may change source locations, subject to the user’s permissions and explicit plan approval.

### 13.3 Secrets

The proposed local scan and review workflow requires no service credentials.

### 13.4 Sensitive Data

Scanning and comparison should require no network connection. Do not upload source text, hashes, fingerprints, filenames, telemetry, or crash reports by default. Avoid logging document contents; permit redaction of paths in diagnostic exports.

Store caches and the database in per-user application locations with appropriate access permissions. Provide cache clearing and state retention controls. Encryption at rest is not established scope; do not claim that fingerprints or local databases are anonymous or encrypted without implementing and validating that protection.

### 13.5 Threats and Mitigations

Opening external files should use platform APIs or structured process arguments. Never execute discovered text/source files, interpret their contents as commands, or construct shell commands by concatenating untrusted paths.

### 13.6 Hardening Checklist

- Verify changing paths, symlink handling, and permissions with filesystem fixtures.
- Test that logging and diagnostic exports respect the content and path policy.
- Exercise partial failure and restart recovery before enabling live cleanup.

> **Sections §14 (Capacity and Scale Assumptions), §15 (Risks), and §16 (Compliance, Licensing, and Data Rights) are Full-tier** and are intentionally omitted at the Standard profile.

## 17. Testing and Acceptance

### 17.1 Definition of Done

A release candidate must demonstrate the approved UI acceptance checks in §11.11 and satisfy the milestone exit conditions in §19. Product acceptance, benchmark targets, and dependency selection remain open where §21 identifies pending decisions.

### 17.2 Test Strategy

#### Development tooling

| Concern | Recommended tool or practice |
| --- | --- |
| Formatting and linting | `cargo fmt` and `cargo clippy`; fail CI on the agreed lint baseline. [S38] |
| Unit/integration tests | `cargo test`; optionally `cargo nextest` for execution, with documentation tests covered separately. [S39] |
| Property tests | `proptest` for normalization, metrics, serialization, and operation invariants. [S40] |
| Benchmarks | Criterion for focused algorithms plus a separate whole-application corpus harness. [S41] |
| Dependency security | `cargo audit` against RustSec advisories. [S42] |
| Dependency policy | `cargo deny` for license/source/advisory policy; review the resolved dependency set against the selected `GPL-3.0-only` distribution terms. [S43] |
| Diagnostics | Structured `tracing` events with content-sensitive redaction. [S24] |
| Continuous integration | GitHub Actions with pinned toolchain and locked dependencies. Linux jobs are release gates; macOS and Windows compile checks are recommended to catch portability regressions early (NFR-008). |

A successful build and unit suite do not demonstrate fuzzy-match quality. The corpus and its expected relationships are release artifacts, not disposable test scaffolding.

#### Labeled corpus

Include exact copies under unrelated names; line-ending/encoding variations; minor edits; inserted and removed passages; reordered sections; short documents contained in long ones; repeated boilerplate; unrelated documents with similar vocabulary; and intentionally misleading near-matches.

Include empty and tiny files, long lines, Unicode, decoding errors, extensionless text, dense duplicate groups, and source/configuration files where punctuation or whitespace matters. Separate tuning and evaluation corpora by source-document family so nearly identical variants do not leak across both sets.

Test partial-overlap detection separately from whole-document resemblance. A single aggregate “accuracy” percentage would conceal important failure modes.

#### Measurements and quality gates

| Area | Measure |
| --- | --- |
| Candidate retrieval | Recall of labeled relationships before final scoring; candidate count and pruning reasons. |
| Final matching | Precision and recall by relation type, profile, and threshold. |
| Evidence | Correctness of displayed shared spans and original-text differences. |
| Performance | Cold/incremental time, bytes read, CPU utilization, peak memory, and cache growth. |
| UI | Input responsiveness, time to first preview, selection stability, large-result scrolling, paired-diff alignment, popup focus/state restoration, and §11.11 acceptance checks. |
| Cancellation | Time to acknowledge and stop bounded work; integrity of interrupted state. |
| Cleanup | Wrong-file operations, missed stale-file detection, partial-failure reporting, and recovery correctness. |

Use progressively larger corpora, for example 1,000, 10,000, and 100,000 files, with both ordinary and pathological distributions. These are proposed benchmark sizes, not promised capacity or speed targets. Record hardware, storage, file-size distribution, cache state, and effective configuration with every result.

Choose numerical quality/performance thresholds after a measured baseline and owner review. Before release, require all labeled exact duplicates to be correctly handled, no wrong-target deletion in the safety suite, no silent destructive fallback, and transparent reporting whenever analysis coverage is incomplete.

#### Safety and recovery tests

Exercise permission failures, source changes during scanning and deletion, symlink replacement, hardlinks, disappearing mounts, non-UTF-8 paths, disk-full errors, Trash rejection, process crashes, and cancellation at each operation stage.

Use temporary test trees and mocked failure injection; no automated destructive test may run against the user's real collection. Include recovery tests for the crash window after a filesystem action but before the database records success.

A future-media readiness test should register a lightweight fake analyzer with a distinct metric payload. It should exercise scheduling, persistence, result presentation routing, and cleanup planning without importing actual media decoders.

### 17.3 Requirement-to-Test Traceability

These are planned checks. Record execution evidence against the canonical ID when verifying the implementation. Exercise destructive and failure paths only in disposable test trees or with a simulated filesystem adapter.

| Requirement ID | Test / Verification Method | Status |
| --- | --- | --- |
| FR-001 | Scan two sibling roots containing renamed copies and also select a nested directory. Check the cross-root group and count each entry once. | Not Started |
| FR-002 | Place one matching and one excluded fixture at each filter boundary. Compare the summary and skip reasons with the selected rules. | Not Started |
| FR-003 | Toggle both policies against hidden and ignored fixtures. Inspect the effective policy and compare discovered entries in each run. | Not Started |
| FR-004 | Scan a temporary tree with a directory symlink loop, FIFO, socket and device fixture. Confirm traversal finishes and reads only eligible files. | Not Started |
| FR-005 | Exercise navigation during a slow scan, request cancellation and check that the acknowledged job shows incomplete coverage. | Not Started |
| FR-006 | Inject permission loss, change a file during reading, remove another and exceed a configured size limit. Check each file’s recorded outcome and the coverage summary. | Not Started |
| FR-007 | Scan the labeled exact corpus under unrelated names. Check every exact pair and verify that hash-only evidence cannot produce a byte-verified label. | Not Started |
| FR-008 | Compare equal decoded text saved with different encodings or line endings. Inspect both the relation label and verification provenance. | Not Started |
| FR-009 | Scan renamed lightly edited revisions alongside unrelated text with a similar filename. Inspect retrieved pairs and the evidence for each. | Not Started |
| FR-010 | Run labeled short-in-long and reordered-section fixtures separately. Check retrieval and the supporting passage evidence for each class. | Not Started |
| FR-011 | Inspect a pair’s named metric, analyzer, profile and verification level. Change a threshold for a new scan and verify that prior results retain their profile. | Not Started |
| FR-012 | Compare originals whose differences disappear under the selected normalization. Locate the differences from the review workspace. | Not Started |
| FR-013 | Use a chain with verified A–B and B–C edges only. Change the reference and verify that no direct A–C score or equivalence is inferred. | Not Started |
| FR-014 | Trigger a candidate cap, timeout, decoding failure and cancellation. Inspect the visible coverage reason for each. | Not Started |
| FR-015 | At the reference window size, inspect additions, removals and changes in both panes. Verify text selection and copying work and editing is unavailable. | Not Started |
| FR-016 | Review two same-named files in different directories. Use the displayed metadata, relation and named score to identify the target in the operation preview. | Not Started |
| FR-017 | Using only the keyboard, navigate groups, protect a member, sort and filter. Verify the same member remains protected from cleanup. | Not Started |
| FR-018 | Queue a row, filter, sort, expand and collapse groups, enter Compare and return. Verify row identity, focus, restored view and queued membership. | Not Started |
| FR-019 | Open and dismiss each popup from its invoking control. Verify the underlying workspace, focus, drafts and queued intent after cancel or close. | Not Started |
| FR-020 | Confirm removal in a disposable tree with one successful and one denied target. Compare both filesystem results with Findings. | Not Started |
| FR-021 | Reject a Trash request in the adapter. Verify the target persists, the failure is visible and no permanent-delete call occurs. | Not Started |
| FR-022 | Replace a target, change a retained copy or modify the plan after preview. Check that affected approval expires and ambiguous targets stay untouched. | Not Started |
| FR-023 | Apply a high threshold and bulk-selection defaults. Exercise each queue entry point and verify fuzzy removal requires deliberate selection and any required unique-text acknowledgment. | Not Started |
| FR-024 | Inject a mixed batch including an interruption after mutation. Compare per-file filesystem evidence, journal entries, cleanup popup and aggregate outcome. | Not Started |
| NFR-001 | Saturate progress delivery with a dense candidate set and a large diff. Record queue depths, peak memory, UI interaction and worker acknowledgment of cancellation. | Not Started |
| NFR-002 | Split tuning and evaluation by source family. Score renamed copies, revisions, containment, reordering, boilerplate and unrelated pairs. Inspect retrieval and final-score reports independently. | Not Started |
| NFR-003 | Inject path replacement, changed identity, missing mounts, permission errors and Trash rejection in temporary trees. Compare intended targets and actual mutations. | Not Started |
| NFR-004 | Scan and compare offline, then observe outbound activity under default settings during the same workflow and diagnostic handling. | Not Started |
| NFR-005 | Exercise both workspaces and popups using only the keyboard, with enlarged text, display scaling, long Unicode paths and visible focus. Check modal shortcut blocking and focus restoration. | Not Started |
| NFR-006 | Benchmark the same corpus cold and incrementally, repeat on a boilerplate-heavy corpus and inspect the recorded environment and measurements. Numerical release budgets await owner review. | Not Started |
| NFR-007 | Measure both regions and their work areas in reference-size captures. Compare typography and compact controls against the current design, then check scaled text for clipping. | Not Started |
| NFR-008 | Review adapter boundaries and search portable components for platform-conditional compilation and direct operating-system calls. Run the Linux adapter suite; where macOS and Windows compile checks exist, confirm the portable components build for those targets. | Not Started |
| IR-001 | Enter Compare from a filtered, sorted, expanded and scrolled Findings table. Return and compare the full view state. | Not Started |
| IR-002 | Toggle wrapping, linked scrolling and next/previous change on unequal-length lines with one-sided insertions. Check alignment, source line numbers and counts. | Not Started |
| IR-003 | Open Settings from New scan. Cancel once and save once, inspect both drafts, then start a new scan and compare the new and previous profiles. | Not Started |
| IR-004 | Try repeated submission, removing every exact copy and changing the approved plan. Inspect adapter calls and verify that analyzer and review paths cannot bypass the service. | Not Started |
| IR-005 | Register a fake analyzer and follow its result through scheduling, storage, preview routing and a simulated plan. Inspect the metric payload and protection checks. | Not Started |
| DR-001 | Cancel after partial progress, restart, save new settings and compare both jobs’ profiles, identities, counts and coverage. | Not Started |
| DR-002 | Round-trip a path with non-UTF-8 bytes on a supporting filesystem. Replace its entry after scanning and check that cleanup rejects the replacement. | Not Started |
| DR-003 | Change one source, normalization profile or analyzer version and inspect invalidated entries. Exercise a decoding error and inspect encoding and error records. | Not Started |
| DR-004 | Round-trip each relation type, inspect both containment directions and change the reference in a nontransitive fuzzy group. | Not Started |
| DR-005 | Interrupt between filesystem mutation and outcome recording. Restart with a replacement at the old path, reconcile and verify that retry leaves completed items and the replacement untouched. | Not Started |
| DR-006 | Inspect default storage after review, exceed the configured cache budget, clear and rebuild. Verify source contents and the pending recovery record survive. | Not Started |
| DR-007 | Migrate a prior supported fixture and restore a backup made during writes. Compare committed scan records, relations and operation history with the consistent snapshot. | Not Started |

## 18. Deployment and Operations

### 18.1 Runtime Environment

V1 is released for Linux only (C-014), with Fedora as the initial validation environment. Avoid Linux-only domain models and keep operating-system behavior behind the adapters required by NFR-008. macOS and Windows compile checks are valuable early but are not V1 release gates. Release support, packaging, signing, and notarization for those platforms are deferred (WH-004).

Choose Linux package formats after the GUI and filesystem permissions are tested. Do not promise a completely static binary. A sandboxed package must be tested with folder selection, mounted volumes, external opening, and Trash behavior—not just application launch.

### 18.2 Configuration

Scan profiles record the selected roots, filters, normalization, thresholds, and resource limits. Settings edits apply to future jobs. Display the effective settings alongside each scan’s results.

### 18.3 Deployment Flow

Ship documented dependency versions, licenses, supported formats, resource limits, and known matching limitations. Pin the Rust toolchain and application dependency lockfile, and update dependencies through reviewed changes rather than unattended major upgrades.

**Owner decision:** Fuzzy DeDupe will use **GNU General Public License version 3.0** and will be written from scratch. GPLv3 is the project's chosen license for its original code; this decision does not depend on inheriting dupeGuru source. The earlier source-derived rewrite plan is superseded.

**Precise license expression:** record the owner's version-3.0 choice as **`GPL-3.0-only`** in package metadata and original-source notices. This means version 3 only. `GPL-3.0-or-later` is a different grant and is not part of this baseline; do not accidentally add “or any later version” by copying a generic notice. [S45]

The implementation and release requirements are:

| Area | Requirement |
| --- | --- |
| Original implementation | Start a new Rust repository. Implement behavior from this specification, the approved UI, algorithm literature, and project-specific tests. dupeGuru is a product/workflow reference, not a codebase to import or translate. |
| License files and notices | Include the full, unmodified GPLv3 text in `LICENSE` and with distributed packages. Preserve applicable copyright, license, and warranty notices. Identify the actual rights holder when establishing original-source notices; the product name alone does not identify that holder. [S44] |
| Rust package metadata | Use `GPL-3.0-only` consistently for project-owned packages. Workspace inheritance may centralize the value. Do not replace third-party license declarations with the project's license. [S45] |
| Downloadable binary releases | Provide equivalent access, at no additional charge, to the corresponding source for each released binary, with clear source directions beside the download. Include the necessary source and build/install scripts under GPLv3's definition; a moving default branch alone does not identify the release's source. Use the distribution method described in GPLv3 section 6(d) for normal downloads. [S44] |
| Dependencies and assets | Review actual dependency versions, enabled features, fonts, icons, and bundled binaries for compatible distribution terms. Retain required third-party notices in the release and record them in `THIRD-PARTY-NOTICES.md`. A from-scratch application may use third-party libraries; their own terms still apply. [S44] |

The selected name, license, and implementation basis are settled. The exact copyright-holder notice and release-specific dependency/source inventory remain implementation records to complete before distribution. This specification records the intended approach; it does not claim that code provenance or a release package has already been audited.

Future FFmpeg packaging requires build-specific review: optional components and configuration affect the resulting license, and some configurations are not redistributable. Running FFmpeg as a sidecar does not automatically eliminate obligations for a binary shipped with the application. [S35]

### 18.5 Observability

Track counts, elapsed time, coverage gaps, warnings, and per-file outcomes. Keep paths redactable and source text out of logs. §17.2 defines the measurements for candidate retrieval, scoring, and resource use.

### 18.6 Backup and Disaster Recovery

Fingerprint indexes and preview caches can be rebuilt. Preserve operation history during cache clearing. A database backup must capture a consistent SQLite snapshot, including committed WAL data. Recovery reconciles uncertain operations before any retry. Restore-time and loss targets have not been set.

### 18.7 Documentation Deliverables

Publish supported formats and platforms, resource limits, matching limitations, license notices, and corresponding-source instructions with the release.

## 19. Implementation Plan

These are dependency-ordered milestones, not time estimates or a commitment to release dates.

| Phase | Deliverable | Exit condition |
| --- | --- | --- |
| MS-0 · Freeze the baseline | Record Fuzzy DeDupe, GPLv3, independent implementation, the plain-text-only V1 format scope, and the Linux-only V1 release. | New repository initialized with the selected license and consistent metadata; owner-confirmed boundaries recorded; excluded features remain excluded. |
| MS-1 · Validate the UI shell | Implement §11: two compact workspaces, large table/paired panes, popup host, stable selection, and background progress. | Fixture-based visual comparison against current Figma, state-restoration tests, and native GUI validation pass; ADR 0001's egui choice is confirmed or superseded. |
| MS-2 · Build discovery and exact matching | File snapshots, bounded traversal, hashes, verification, basic persistence. | Exact fixtures and filesystem edge cases pass without live cleanup enabled. |
| MS-3 · Evaluate text similarity | Normalization, candidate retrieval alternatives, scoring, labeled corpus. | Measured precision/recall and cost justify selected profiles and algorithms. |
| MS-4 · Integrate review | Table-backed groups, paired evidence, original-text previews, popup settings, cache validity, cancellation. | End-to-end discovery-to-review works without hidden coverage assumptions; current scan profiles remain immutable. |
| MS-5 · Enable controlled cleanup | Plan validation, Trash backend, journaling, failure injection, recovery. | Safety suite passes before exposing real deletion in a release build. |
| MS-6 · Harden and package | Migration tests, performance baselines, dependency/license review, Linux release testing, and corresponding-source packaging. | Documented release criteria are satisfied; binary/source versions match; required license and third-party notices are included. |
| Later media phases | Add one analyzer and reviewer at a time. | Each passes its own accuracy/resource/packaging gate without weakening shared safety. |

Do not select text algorithms solely because they are easy to implement, and do not implement all future formats before demonstrating that V1 solves the motivating text workflow.

> **§20 (Success Evaluation) is Full-tier** and is intentionally omitted at the Standard profile.

## 21. Open Questions and Decisions

This section preserves the decision register and principal risks in §§17–18 of `docs/specs/fuzzy_dedupe_spec.md` (version 0.3, November 19, 2026). Canonical question IDs `OQ-001`–`OQ-012` correspond in order to the twelve decisions in the original register; the original IDs remain in the question text for traceability. Answered decisions remain recorded for continuity and are not reopened by unfinished release work.

An em dash means the source did not assign an owner, deadline, or blocking status. Each validation gate applies to its own decision.

### 21.1 Decision register

| ID | Question | Current Assumption | Blocking? | Owner | Needed By | Status |
| --- | --- | --- | --- | --- | --- | --- |
| OQ-001 | What is the project name? (Source decision 01.) | **Confirmed:** Fuzzy DeDupe. Apply the name to the application and project documentation. | No; answered. | — | Answered; application and documentation naming use this choice. | Answered |
| OQ-002 | Does V1 include extracted document formats as well as plain text? (Source decision 02.) | **Confirmed:** V1 is plain text only. PDF, DOCX, ODT, and other extracted formats are excluded from V1 and deferred (C-013, WH-003). | No; answered. | — | Answered 2026-11-21. | Answered |
| OQ-003 | Which operating systems receive initial release support? (Source decision 03.) | **Confirmed:** V1 is released for Linux only, developed so that macOS and Windows support can be added later (C-014, NFR-008, WH-004). | No; answered. | — | Answered 2026-11-21. | Answered |
| OQ-004 | Does egui/eframe satisfy the approved compact UI in a native implementation? (Source decision 04.) | **Confirmed:** egui/eframe ([ADR 0001](../../adr/adr-0001-use-egui-for-the-v1-desktop-ui.md)), final once the MS-1 prototype passes; Slint is the fallback. | No; answered. | — | Answered 2026-11-21. | Answered |
| OQ-005 | Which fuzzy algorithms, indexes, parameters, defaults, and quality targets will be selected? (Source decision 05.) | Benchmark document resemblance and passage containment separately. Algorithms and their defaults remain choices to validate rather than settled conclusions. | — | — | Before freezing the matching design and its defaults; no date specified. | Open |
| OQ-006 | Which tokenization and normalization profiles will be used? (Source decision 06.) | Conservative defaults, visible settings, and versioned artifacts are the recommended approach; final profiles remain open. | — | — | — | Open |
| OQ-007 | What engine policy defines result grouping and group/category counts? (Source decision 07.) | Preserve reference-relative pair evidence in a grouped table. Group/category-count semantics still require an explicit engine policy; the approved table layout does not settle that policy. | — | — | Before relying on group/category-count semantics; no date specified. | Open |
| OQ-008 | Will V1 include rename, move, or copy operations? (Source decision 08.) | These are an optional V1 extension after the essential review/delete workflow. Their inclusion is not confirmed. | Unspecified in source; these operations are optional. | — | After the essential review/delete workflow, if the extension is included. | Open |
| OQ-009 | Which Trash backend will be used, and how will permanent deletion be exposed? (Source decision 09.) | Require a backend safety gate, fail closed without Trash, and require separate confirmation for permanent deletion. Backend selection and permanent-delete exposure remain open. | Yes, for accepting the backend without its safety gate; fail closed without Trash. | — | At the backend safety gate, before relying on it for cleanup. | Open |
| OQ-010 | What resource limits and performance targets will apply? (Source decision 10.) | Select limits and targets from representative measurements, not assumptions about workstation hardware. No measured guarantee is available. | — | — | After representative measurements, before committing measured limits or targets. | Open |
| OQ-011 | In what order will future media formats be added? (Source decision 11.) | Add formats incrementally. No release dates or universal score contract are established. | Unspecified in source; future formats are outside the text-only V1. | — | When selecting later media increments; no date specified. | Open |
| OQ-012 | What release notices and records remain after the implementation basis and license decisions? (Source decision 12.) | **Basis and license answered:** a from-scratch Rust application under GPLv3, recorded as `GPL-3.0-only`. Only the actual copyright-holder notice, dependency inventory, and corresponding-source release records remain to be completed before distribution. | Yes, for distribution until the remaining release records are complete. | — | Before distribution. | Open |

OQ-012 is open only for its remaining distribution work. It does not reopen the confirmed from-scratch implementation or license. dupeGuru is a product/workflow reference; the earlier clone/port proposal no longer defines the implementation. No permission to use later GPL versions is assumed.

### 21.2 Principal risks and required mitigation or evidence

These are the source's nine principal risks. Their inclusion records required mitigation or evidence, not proof that the risk has already been controlled. No additional likelihood, severity, owner, or acceptance threshold is assigned here.

| Risk | Mitigation or evidence required |
| --- | --- |
| Fuzzy matches are mistaken for redundant information. | Show original differences and directional coverage; do not automatically delete fuzzy matches. |
| Approximate retrieval misses valid pairs. | Measure retrieval recall and disclose profile/index limitations. |
| Common text produces enormous candidate groups. | Use bounded indexing and explicit partial-coverage reporting; benchmark boilerplate-heavy corpora. |
| Stale scans or path replacement target the wrong file. | Revalidation, identity checks, approval invalidation, and recovery tests. |
| Encoding or normalization creates false equivalence. | Record decoding errors and profile transformations; preserve original review. |
| The GUI becomes unusable at scale. | Virtualization, lazy previews, background work, stable selection, and a prototype gate. |
| Video introduces excessive CPU or memory consumption. | Per-decoder resource limits and process supervision when the format is added. |
| A text-specific schema obstructs new media. | Versioned analyzer artifacts and typed media-specific evidence under a common envelope. |
| “Rust” is mistaken for complete safety. | Review FFI, system APIs, codecs, concurrency, and destructive-operation behavior. |

### 21.3 Confirmed direction, recommendations, and remaining uncertainty

The confirmed product and implementation direction is **Fuzzy DeDupe**, written from scratch in **Rust** under **GPLv3 (`GPL-3.0-only`)**. The approved interaction model is a table-dominant **Findings** workspace, a large paired-pane **Compare** workspace, and compact popups for supporting tasks, as specified in §11. These choices remain resolved while GUI framework fit is validated.

Source §18 recommends a **text-only V1 with read-only review**, centered on discovering and explaining duplicate relationships and safely removing user-selected files. “Read-only” describes document review, not an exclusion of the cleanup workflow. A reusable local core, SQLite persistence, and bounded background processing remain the recommended foundation. egui/eframe is selected by ADR 0001 and becomes final once the MS-1 prototype passes. The similarity algorithms require benchmark validation; this section does not promote them to confirmed choices.

Design the shared infrastructure for multiple media types now, while implementing only the text analyzer. Later formats should extend decoding, fingerprints, indexes, metrics, and preview renderers without replacing the cleanup safety model. Incremental media expansion does not establish release dates or a universal similarity score.

The owner has since confirmed a plain-text-only V1 and a Linux-only V1 release with an architecture portable to macOS and Windows. The owner also selected egui/eframe and excluded assistive-technology support. The remaining unresolved issues are egui's fit in the MS-1 prototype, algorithm parameters and quality targets, optional file-management breadth, and the Trash backend. **No throughput, recall, memory, or recovery guarantee has yet been demonstrated on an implementation.**

The retained product vision is to find identical files, discover substantially overlapping text, show what differs, and let the user clean up with clear evidence and controlled actions, without editing documents or relying on AI.

## Deviations Log

No implementation deviations have been recorded.

## References

The [original v0.3 draft](../fuzzy_dedupe_spec.md) preserves the source numbering and historical product decisions.

Technical primary references below were recorded during the original research on **November 11, 2026**. Version 0.2 inspected the live compact Figma nodes and local planning documents. Version 0.3 records the owner's name, license, and implementation decisions and checks the GPLv3/SPDX references on **November 19, 2026**; it does not re-audit the earlier package or Figma findings. Links using `latest` or a repository's default branch can change; implementation records should capture the actual selected versions and commits. The datasketch references explain algorithms and their tradeoffs; they are not a recommendation to add a Python runtime.

| Topic | References |
| --- | --- |
| Approved compact UI and popup implementation reference | [Findings][UX01]; [prototype][UX02]; [comparison][UX03]; [compact page][UX04]; [settings][UX05]; [new scan][UX06]; [cleanup][UX07]; [unique-text review][UX08]; [components][UX09]; [comparison prototype][UX10] |
| Product/workflow reference | [dupeGuru repository][S01] |
| Selected project license and release requirements | [GNU GPLv3 full text, especially sections 1 and 4–6][S44]; [SPDX GPL-3.0-only identifier and notice][S45] |
| Recommended GUI | [egui project and limitations][S03]; [egui_extras TableBuilder][S04] |
| GUI alternatives | [Slint][S05]; [Iced documentation][S06]; [Tauri prerequisites][S07]; [CXX-Qt book][S08] |
| Discovery and fingerprints | [ignore][S09]; [BLAKE3][S10]; [xxhash-rust][S11] |
| Text processing | [encoding_rs][S12]; [Unicode normalization][S13]; [Unicode segmentation][S14]; [RapidFuzz][S15]; [similar][S16] |
| Background processing | [Rayon][S17]; [Crossbeam channels][S18] |
| Persistence | [rusqlite and bundled SQLite][S19]; [refinery][S20]; [SQLite WAL behavior][S21] |
| Desktop integration | [rfd][S22]; [trash, including platform cautions][S23] |
| Diagnostics and settings | [tracing][S24]; [Serde][S25]; [TOML][S26] |
| Text similarity methods | [MinHash][S27]; [MinHash LSH and approximation][S28]; [Winnowing author page and errata][S29] |
| Future images | [image][S30]; [image_hasher][S31] |
| Future audio | [Symphonia][S32]; [Chromaprint][S33] |
| Future video | [ffmpeg-sidecar][S34]; [FFmpeg license and configuration][S35] |
| Architectural precedent and storage accounting | [Czkawka/Krokiet repository][S36]; [Btrfs extent sharing][S37] |
| Development quality | [Clippy][S38]; [Nextest][S39]; [Proptest][S40]; [Criterion][S41]; [RustSec/cargo-audit][S42]; [cargo-deny][S43] |

[S01]: https://github.com/arsenetar/dupeguru 'dupeGuru product/workflow reference'
[S03]: https://github.com/emilk/egui 'egui project'
[S04]: https://docs.rs/egui_extras/latest/egui_extras/struct.TableBuilder.html 'egui_extras TableBuilder'
[S05]: https://slint.dev/ 'Slint'
[S06]: https://docs.rs/iced/latest/iced/ 'Iced documentation'
[S07]: https://v2.tauri.app/start/prerequisites/ 'Tauri prerequisites'
[S08]: https://kdab.github.io/cxx-qt/book/ 'CXX-Qt documentation'
[S09]: https://docs.rs/ignore/latest/ignore/ 'ignore directory traversal'
[S10]: https://docs.rs/blake3/latest/blake3/ 'BLAKE3 Rust implementation'
[S11]: https://docs.rs/xxhash-rust/latest/xxhash_rust/ 'xxhash-rust'
[S12]: https://docs.rs/encoding_rs/latest/encoding_rs/ 'encoding_rs'
[S13]: https://docs.rs/unicode-normalization/latest/unicode_normalization/ 'Unicode normalization'
[S14]: https://docs.rs/unicode-segmentation/latest/unicode_segmentation/ 'Unicode segmentation'
[S15]: https://docs.rs/rapidfuzz/latest/rapidfuzz/ 'RapidFuzz Rust'
[S16]: https://docs.rs/similar/latest/similar/ 'similar diffing library'
[S17]: https://docs.rs/rayon/latest/rayon/ 'Rayon'
[S18]: https://docs.rs/crossbeam-channel/latest/crossbeam_channel/ 'Crossbeam channels'
[S19]: https://github.com/rusqlite/rusqlite 'rusqlite and bundled SQLite'
[S20]: https://docs.rs/refinery/latest/refinery/ 'refinery migrations'
[S21]: https://sqlite.org/wal.html 'SQLite write-ahead logging'
[S22]: https://docs.rs/rfd/latest/rfd/ 'Rusty File Dialogs'
[S23]: https://docs.rs/trash/latest/trash/ 'trash platform behavior and safety caveats'
[S24]: https://docs.rs/tracing/latest/tracing/ 'tracing'
[S25]: https://docs.rs/serde/latest/serde/ 'Serde'
[S26]: https://docs.rs/toml/latest/toml/ 'TOML'
[S27]: https://ekzhu.com/datasketch/minhash.html 'MinHash explanation'
[S28]: https://ekzhu.com/datasketch/lsh.html 'MinHash LSH and approximation tradeoffs'
[S29]: https://sschleimer.warwick.ac.uk/Maths/winnowing.html 'Winnowing author page and errata'
[S30]: https://docs.rs/image/latest/image/ 'image decoding and manipulation'
[S31]: https://docs.rs/image_hasher/latest/image_hasher/ 'Perceptual image hashing'
[S32]: https://github.com/pdeljanov/Symphonia 'Symphonia audio decoding'
[S33]: https://acoustid.org/chromaprint 'Chromaprint'
[S34]: https://github.com/nathanbabcock/ffmpeg-sidecar 'FFmpeg sidecar integration'
[S35]: https://github.com/FFmpeg/FFmpeg/blob/master/LICENSE.md 'FFmpeg license and configuration'
[S36]: https://github.com/qarmin/czkawka 'Czkawka and Krokiet'
[S37]: https://btrfs.readthedocs.io/en/stable/dev/dev-btrfs-design.html 'Btrfs extent sharing and snapshots'
[S38]: https://doc.rust-lang.org/clippy/ 'Clippy'
[S39]: https://github.com/nextest-rs/nextest 'Nextest'
[S40]: https://proptest-rs.github.io/proptest/intro.html 'Proptest'
[S41]: https://bheisler.github.io/criterion.rs/book/ 'Criterion.rs'
[S42]: https://rustsec.org/ 'RustSec and cargo-audit'
[S43]: https://embarkstudios.github.io/cargo-deny/ 'cargo-deny'
[UX01]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=13-648 'Approved compact findings table'
[UX02]: https://www.figma.com/proto/d5tA6ap1tH4cTDaJiwve89?node-id=13-648&starting-point-node-id=13%3A648 'Compact findings-to-comparison prototype'
[UX03]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=13-1304 'Approved large paired comparison panes'
[UX04]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=13-529 'Current page: 01 · Compact desktop'
[UX05]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=16-735 'Settings popup'
[UX06]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=16-806 'New scan popup'
[UX07]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=16-886 'Cleanup review popup'
[UX08]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=16-932 'Unique-text review popup'
[UX09]: https://www.figma.com/design/d5tA6ap1tH4cTDaJiwve89?node-id=13-538 'Compact shared components'
[UX10]: https://www.figma.com/proto/d5tA6ap1tH4cTDaJiwve89?node-id=13-1304&starting-point-node-id=13%3A1304 'Large side-by-side comparison prototype'
[S44]: https://www.gnu.org/licenses/gpl-3.0.html 'GNU General Public License version 3'
[S45]: https://spdx.org/licenses/GPL-3.0-only.html 'SPDX GNU General Public License version 3 only'

## Appendix A: ID Conventions

Stable IDs allow requirements to be referenced from commits, tests, issues, ADRs, and review comments — and let an implementer's completion claims be mechanically checked. Section numbers below match `spec-full-template.md`, so an ID keeps the same "Defined In" reference across every profile.

| Prefix | Meaning                     | Defined In     |
| ------ | --------------------------- | -------------- |
| `G-`   | Goal                        | §4             |
| `NG-`  | Non-goal (never)            | §2.2           |
| `WH-`  | Won't have in v1 (deferred) | §2.3           |
| `A-`   | Assumption                  | §3.3           |
| `C-`   | Constraint                  | §3.4           |
| `FR-`  | Functional requirement      | §7.1           |
| `NFR-` | Non-functional requirement  | §7.2           |
| `IR-`  | Interface requirement       | §7.3           |
| `DR-`  | Data requirement            | §7.4           |
| `D-`   | Design decision             | §8.3           |
| `AW-`  | Alternate workflow          | §10.2          |
| `EC-`  | Edge case                   | §10.3          |
| `ERR-` | Error-handling requirement  | §12.1          |
| `MS-`  | Milestone                   | §19            |
| `OQ-`  | Open question               | §21            |
| `DEV-` | Deviation                   | Deviations Log |

The `R-` (Risk) prefix is Full-tier (§15) and is not used at the Standard profile. Priority values (`Must/Should/Could`) are column values, not ID prefixes — IDs never change when priorities do.

---

## Appendix B: Agent Implementation Contract

Binding when this spec is implemented by a coding agent. (Applies equally well to human contractors.)

### B.1 Implementation Rules

The implementer shall:

- Read this entire specification before making changes; per session thereafter, re-read at minimum §7 (Requirements), §21 (Open Questions), and the Deviations Log — Background and References may be read once.
- Preserve all explicit non-goals, won't-haves, constraints, and design constraints.
- Treat **Must** requirements as mandatory and **blocking** open questions as hard stops for the affected work.
- On encountering underspecified behavior: file an `OQ-` row **with a proposed default assumption** and proceed on it only if non-blocking — never guess silently.
- On any divergence from the spec: record a `DEV-` row (spec reference, what, why) rather than adapting silently.
- Add or update tests for every implemented requirement; keep §17.3 (traceability) current.
- Follow the milestone order in §19; do not build later milestones on unproven earlier ones.
- Prefer small, reviewable changes; avoid broad refactors unless the spec requires them.
- Document any discovered mismatch between the spec and existing code as a `DEV-` or `OQ-` row.

### B.2 Prohibited Behaviors

The implementer shall not:

- Invent requirements not present in this spec.
- Remove existing behavior unless explicitly required.
- Introduce external services or dependencies not agreed with the owner without an approved `OQ-`.
- Store secrets in source control or print them in CI logs.
- Ignore failing tests unrelated to the change without documenting them.
- Treat examples as exhaustive or normative unless explicitly stated.
- Mark a requirement complete without a verification entry in §17.3.

### B.3 Required Completion Report (verification gate)

At completion, provide:

- Summary of changes and files changed.
- **Requirements implemented, each mapped to the test or command that proves it** — i.e., the completed §17.3 matrix. Claims without verification entries are not accepted.
- Tests added or changed.
- Deviations (`DEV-` rows) and their approval status.
- Known limitations and remaining open questions.
- Documentation deliverables completed (§18.7).

### B.4 Session Handoff

For multi-session implementations: record current milestone, in-progress requirement IDs, and unresolved `OQ-`/`DEV-` items in the repository's session-state/handoff documents at the end of each session, per the repo's documentation convention. The spec records _what and why_; handoff docs record _where work stands_.

---

> **Appendix C (Optional Modules) is Full-tier** — external-integration, scheduling, entity-resolution, and scoring modules — and is intentionally omitted at the Standard profile.

## Appendix D: Tailoring

Pick the smallest profile that fits; upgrade if the project grows. Because numbering is stable across profiles, upgrading is **additive**: insert the missing sections at their canonical numbers and set `profile:` in the frontmatter — no existing section or ID reference changes.

| Profile | Template File | Use For |
| --- | --- | --- |
| **Light** | `spec-light-template.md` | Scripts, small tools, single-session agent tasks |
| **Standard** | this file | Typical features and services |
| **Full** | `spec-full-template.md` | Multi-service systems, durable data, external integrations, or multiple stakeholders |

Rules of thumb:

- Owns durable data → §18.6 Backup/DR is required.
- Talks to external paid/rate-limited APIs, makes automated decisions users must trust, or spans multiple services/stakeholders → upgrade to **Full** (adds §5, §8.4, §8.6, §14–§16, §18.4, §20, §19 Waves, and Appendix C modules).
- Implemented by a coding agent → Appendix B is required (it is the cheapest section and the highest-leverage one).
