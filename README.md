# Fedora CoreOS updates backend

[![Build Status](https://api.travis-ci.com/coreos/fedora-coreos-cincinnati.svg?branch=master)](https://travis-ci.com/coreos/fedora-coreos-cincinnati)
![minimum rust 1.37](https://img.shields.io/badge/rust-1.37%2B-orange.svg)

This repository contains the logic for Fedora CoreOS auto-updates backend.

This service provides an implementation of the [Cincinnati protocol][cincinnati], which is consumed by on-host update agents (like [Zincati][zincati]).

This workspace can be built with `cargo build` and contains the following binaries:

 * `dumnati`: initial development stub

[cincinnati]: https://github.com/openshift/cincinnati
[zincati]: https://github.com/coreos/zincati
