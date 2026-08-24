# workerd config for the conformance suite (M2). Run from tests/conformance:
#   workerd serve workerd-conformance.capnp
#
# T4 (fetch Host-parity): the worker's globalOutbound is routed to a local
# echo origin standing in for api.example.com; ExternalServer forwards the
# request keeping the original Host header, which is what the T4 assertions
# check (D5.1).

using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    ( name = "main", worker = .worker ),
    ( name = "echo-origin",
      external = ( address = "127.0.0.1:18080", http = () ) ),
  ],
  sockets = [
    ( name = "http", address = "127.0.0.1:8788", http = (), service = "main" ),
  ],
);

const worker :Workerd.Worker = (
  compatibilityDate = "2025-08-01",
  modules = [
    (name = "index.js", esModule = embed "build/index.js"),
    (name = "index_bg.wasm", wasm = embed "build/index_bg.wasm"),
  ],
  globalOutbound = "echo-origin",
);
