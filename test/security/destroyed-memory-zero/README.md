# Destroyed-memory zeroization test

A TVM's confidential pages stay converted after `DESTROY_TVM`, so the TSM can
hand them to the next TVM without the host reclaiming them. This test checks
that no data survives that reuse.

The host converts the metadata and guest RAM once, then twice creates, runs and
destroys a TVM on the same physical pages without reclaiming in between. The
guest scans a 1 MiB window mapped with `ADD_ZERO_PAGES`, then fills it with
`0xa5` and shuts down. The second TVM's zero pages are the first one's window,
so they read as zero only if destroying the first TVM cleared them.

On a leak the guest prints `[TVM] LEAK` and halts instead of shutting down: the
host never regains control and the test runner reports a timeout.

```sh
make -C test/security/destroyed-memory-zero run
```

Expected final output:

```text
[TVM] zero pages are zero; planting secret
[HOST] PASS: destroyed TVM data never reaches the next TVM
```
