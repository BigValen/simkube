<!--
project: SimKube
template: docs.html
-->

# skctl

`skctl` is the CLI for interacting with SimKube.  It's not required to use but it will make your life a lot easier.

```
command-line app for running simulations with SimKube

Usage: skctl <COMMAND>

Commands:
  crd     print SimKube CRDs
  delete  delete a simulation
  export  export simulation trace data
  run     run a simulation
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## skctl crd

```
print SimKube CRDs

Usage: skctl crd

Options:
  -h, --help     Print help
  -V, --version  Print version
```

Generate all of the necessary CustomResourceDefinitions for SimKube.

## skctl delete

```
delete a simulation

Usage: skctl delete --name <NAME>

Options:
      --name <NAME>
          name of the simulation to run

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## skctl export

```
export simulation trace data

Usage: skctl export [OPTIONS]

Options:
      --start-time <START_TIME>
          trace export start timestamp; can be a relative duration
                or absolute timestamp; durations are computed relative
                to the specified end time, _not_ the current time

          [default: -30m]

      --end-time <END_TIME>
          end time; can be a relative or absolute timestamp

          [default: now]

      --excluded-namespaces <EXCLUDED_NAMESPACES>
          namespaces to exclude from the trace

          [default: cert-manager,kube-system,local-path-storage,monitoring,simkube]

      --tracer-address <TRACER_ADDRESS>
          sk-tracer server address

          [default: http://localhost:7777]

      --output <OUTPUT>
          location to save exported trace

          [default: file:///tmp/kind-node-data]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

```

Export a trace from a running `sk-tracer` pod between the specified `--start-time` and `--end-time`, as well as
according to the specified filters.  The resulting trace will be stored in the `--output` directory.  Timestamps
can either be relative ("-2h", "now", etc) or absolute ("2024-01-01T12:00:00").  If you find a timestamp format
that isn't accepted or is parsed incorrectly, please [file an issue](https://github.com/acrlabs/simkube/issues/new?template=bug_report.md&title=incorrect%20timestamp%20parsing&labels=cli,bug).

## skctl run

```
run a simulation

Usage: skctl run [OPTIONS] --name <NAME>

Options:
      --name <NAME>
          name of the simulation to run

      --driver-namespace <DRIVER_NAMESPACE>
          namespace to launch sk-driver in

          [default: simkube]

      --trace-file <TRACE_FILE>
          location of the trace file for sk-driver to read

          [default: file:///data/trace]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
