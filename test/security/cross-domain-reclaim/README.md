# Cross-domain reclaim

This test checks that a supervisor domain cannot reclaim confidential memory
converted by another supervisor domain.

The platform contains two untrusted supervisor domains, both present in the
TSM trust map. The device-tree extension `device-tree.dts.S` adds the attacker
domain on hart 1, the same way as `trust-map-enforcement`.

The host converts 4 MiB and publishes the range through a coordination page.
The attacker then calls `COVH_RECLAIM_PAGES` on that range. Shadowfax must
reject the call because the attacker does not own the allocation. The host
then reclaims the range itself, which must succeed.

Build and run with:

```sh
make -C test/security/cross-domain-reclaim run
```

A successful run ends with:

```text
[ATTACKER] PASS: reclaim of host-owned pages was rejected
[HOST] RECLAIM_PAGES OK
[HOST] PASS: only the owner could reclaim its confidential pages
```
