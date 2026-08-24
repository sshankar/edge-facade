# workerd config for the Edge SDK conformance runs (M2).
# Run (from examples/hello-world): workerd serve workerd-hello-world.capnp

using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    ( name = "main", worker = .worker ),
  ],
  sockets = [
    ( name = "http", address = "127.0.0.1:8787", http = (), service = "main" ),
  ],
);

const worker :Workerd.Worker = (
  compatibilityDate = "2025-08-01",
  modules = [
    (name = "index.js", esModule = embed "build/index.js"),
    (name = "index_bg.wasm", wasm = embed "build/index_bg.wasm"),
  ],
);
