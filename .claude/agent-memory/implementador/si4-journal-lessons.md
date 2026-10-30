---
name: si4-journal-lessons
description: M3-2b design facts: journal .txn, consumed Si4Writer, FailPoint semantics, clippy as_chunks
metadata:
  type: project
---

- Transactions run `build` closure that must MOVE and drop writer handles (lock + Windows renames) before commit; `run_transaction` borrows the stem, so clone it before moving into the closure.
- Injected failures skip all cleanup (simulate kill); real errors before the "journal" stage remove the .tmp files.
- Reader (`Si4Database::open`) must never delete .tmp: a live writer may own them. Only `.txn` makes it fail (`PendingJournal`).
- clippy 1.99 `chunks_exact(CONST)` -> use `as_chunks::<N>().0`; `case_sensitive_file_extension_comparisons` hits `ends_with(".tmp")` in tests.
- Tester's tests/ files may be mid-edit (transient failures there are not mine).
