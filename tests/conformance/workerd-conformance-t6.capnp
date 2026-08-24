# workerd config for the conformance suite's T6 (error-surface parity).
# Run from tests/conformance:
#   workerd serve workerd-conformance-t6.capnp
#
# Unlike workerd-conformance.capnp, this instance's globalOutbound points at
# a port where nothing listens (127.0.0.1:19999), so every worker fetch
# rejects at the network level. The T6 handler reports the FetchError
# category; D16 says CF surfaces these as `Connection`, which is what the
# driver asserts. Serves the same build/index.js as the main instance.

using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    ( name = "main", worker = .worker ),
    ( name = "dead-outbound",
      external = ( address = "127.0.0.1:19999", http = () ) ),
  ],
  sockets = [
    ( name = "http", address = "127.0.0.1:8789", http = (), service = "main" ),
  ],
);

const worker :Workerd.Worker = (
  compatibilityDate = "2025-08-01",
  modules = [
    (name = "index.js", esModule = embed "build/index.js"),
    (name = "index_bg.wasm", wasm = embed "build/index_bg.wasm"),
  ],
  globalOutbound = "dead-outbound",
);
