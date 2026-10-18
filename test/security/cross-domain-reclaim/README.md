# Cross-domain interference

This test checks that a supervisor domain cannot operate on a TVM, or on
confidential memory, owned by another supervisor domain.

The platform contains two untrusted supervisor domains, both present in the
TSM trust map. The test's `device-tree.dts` adds the attacker domain on hart 1,
as `trust-map-enforcement` does. Unlike that test, the attacker is a legitimate
TSM client, so only ownership checks can stop it.

The host converts metadata and guest RAM and creates a TVM. As the TVM moves
through its lifecycle, the attacker makes three attempts against it:

| Host state | Attacker attempt | Effect if it succeeds |
|------------|------------------|-----------------------|
| TVM created | `COVH_ADD_MEMORY_REGION` on the host TVM | tampers with the TVM's memory layout |
| TVM stopped | `COVH_DESTROY_TVM` on the host TVM | destroys the host's TVM (denial of service) |
| TVM destroyed | `COVH_RECLAIM_PAGES` on the host pages | steals the host's pages and reads them |

The attacker records each outcome separately, and the host finishes its own
lifecycle either way, so one run reports all three. The test passes only if
every attempt is rejected and the host can still destroy its TVM and reclaim
its pages.

Build and run with:

```sh
make -C test/security/cross-domain-reclaim run
```

A successful run ends with:

```text
[HOST] blocked: add a memory region to the host TVM
[HOST] blocked: destroy the host TVM
[HOST] blocked: reclaim the host pages
[HOST] PASS: another domain could not touch the host TVM or its pages
```
