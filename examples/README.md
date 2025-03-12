# Shadowfax examples
These are dummy examples that tests working setup with Qemu and prints helloworld messages
to Qemu UART. In future, these examples will use shadowfax functions.

## Running rust example
Since Cargo supports for example, you can execute the following command from the root directory:

```sh
cargo run --example helloworld-rust
```

## Running C example
You can run the C example either from the root directory either from `examples/helloworld-c`
(use the `-C` flag for the `make` command).

```sh
make -C examples/helloworld-c CROSS_COMPILE=<your-toolchain-prefix> run
```
