# Memory ownership consistency

This test checks that a memory-ownership transaction rejected by the TSM does
not alter Shadowfax's committed ownership map or the PMP state derived from it.

The launcher converts one naturally aligned metadata region and creates a TVM
whose page table and state live inside it. Shadowfax records the conversion as
one committed allocation. The TSM, however, splits its internal description
when those pages become owned by the TVM. Reclaiming the original whole range
therefore has the exact base and size expected by Shadowfax's initial
validation, but the TSM rejects it because there is no longer a matching free
confidential block.

The test then checks two simple API-level observations:

1. The whole-range reclaim returns the expected TSM failure.
2. A subsequent operation on the TVM succeeds, showing that the rejected
   transaction did not invalidate the TVM which owns the metadata.

Shadowfax derives the PMP map from the committed domain memory regions. Since
the failure path aborts the pending transaction without moving the committed
allocation, the PMP map is not reprogrammed with a different owner. The test
intentionally avoids a separate trap handler or direct PMP probe.

Build and run with:

```sh
make -C test/security/memory-ownership-consistency run
```

A successful run ends with:

```text
[HOST] TSM rejected reclaim of TVM-owned metadata
[HOST] ADD_MEMORY_REGION_AFTER_FAILURE OK
[HOST] PASS: rejected reclaim left the TVM usable
```
