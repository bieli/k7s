# k7s
Kubernetes dashboard lite - RUST thin kubectl replacer and maybe k9s in next iterations ;-) Let's dance!

```bash
██╗  ██╗███████╗ █████╗
██║ ██╔╝╚════██║██╔══╝
█████╔╝     ██╔╝╚████╗
██╔═██╗    ██╔╝  ╚══██╗
██║  ██╗   ██║  █████╔╝
╚═╝  ╚═╝   ╚═╝  ╚════╝
```

## First ALPHA version view - terminal UI

![](https://raw.githubusercontent.com/bieli/k7s/master/assets/screenshot_01.v0.1.0-alpha.png)


## How to run this app.?

### Setup kube config env.

Based on kube config file, internal K8s API client can "speak" with your kubernetes cluster.

```bash
export KUBECONFIG=/home/bieli/.kube/config
```

### Compile and run

```bash
cargo run --release
```

or you can build binary

```bash
cargo build --release
```


