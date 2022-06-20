# Quickstart Guide

Clone the repo, then open the directory in terminal. Run both these commands in parallel (in two terminals):

```
cargo run --bin fcos-graph-builder -- -c dist/fcos-graph-builder.toml.sample

cargo run --bin fcos-policy-engine -- -c dist/fcos-policy-engine.toml.sample
```

These commands already provided a sample configuration file (dist/fcos-policy-engine.toml.sample and dist/fcos-graph-builder.toml.sample) but if you would like to have your own config file then you can edit/replace these files. 

To test if your graph-builder is up and running you can curl your localhost:8080 port, for example:
```
curl -H 'Accept: application/json' 'http://localhost:8080/v1/graph?basearch=x86_64&stream=stable'
``` 

The policy-engine can be tested by using curl on your localhost:8081 port, for example:
```
curl -H 'Accept: application/json' 'http://localhost:8081/v1/graph?basearch=x86_64&stream=stable&rollout_wariness=0'
```