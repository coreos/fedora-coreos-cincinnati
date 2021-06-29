# Fedora CoreOS updates backend

![Checks status](https://img.shields.io/github/checks-status/coreos/fedora-coreos-cincinnati/main)
![minimum rust 1.39](https://img.shields.io/badge/rust-1.39%2B-orange.svg)

This repository contains the logic for Fedora CoreOS auto-updates backend.

This service provides an implementation of the [Cincinnati protocol][cincinnati], which is consumed by on-host update agents (like [Zincati][zincati]).

This workspace can be built with `cargo build` and contains the following binaries:

 * `fcos-graph-builder`: a service which builds and caches the raw update graph
 * `fcos-policy-engine`: a web service which handles requests from agents

[cincinnati]: https://github.com/openshift/cincinnati
[zincati]: https://github.com/coreos/zincati
