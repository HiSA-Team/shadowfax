# Trust-map enforcement

This test verifies that a supervisor domain omitted from a TSM's trust map
cannot discover or invoke that TSM.

The platform contains three non-root supervisor domains:

- the TSM domain;
- the normal untrusted domain, which is present in the TSM trust map;
- an attacker domain, which is not present in the trust map.

The attacker first calls `SUPD_GET_ACTIVE_DOMAINS`. The call itself succeeds,
but the returned bitmap must be zero: the extension only returns TSM domains
that trust the caller, and the root domain is never included. The attacker then
guesses the known TSM domain ID and directly issues `COVH_GET_TSM_INFO` with an
otherwise valid, page-aligned buffer. Shadowfax must reject the call before
entering the TSM because the attacker is absent from its trust map.

Build and run with:

```sh
make -C test/security/trust-map-enforcement run
```

A successful run ends with:

```text
[ATTACKER] PASS: SUPD enumeration returned no domains
[ATTACKER] PASS: guessed TSM call was rejected
[HOST] PASS: untrusted domain could not discover or call the TSM
```
