![github_tag](https://img.shields.io/github/v/tag/bieli/k7s)
[![Crates.io](https://img.shields.io/crates/v/k7s.svg)](https://crates.io/crates/k7s)
[![Quality Gate Status](https://sonarcloud.io/api/project_badges/measure?project=bieli_k7s&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=bieli_k7s)
# k7s - Kubernetes Resources Viewer
RUST thin kubectl replacer and maybe k9s in next iterations ;-) Let's dance!

```bash
██╗  ██╗███████╗ █████╗
██║ ██╔╝╚════██║██╔══╝
█████╔╝     ██╔╝╚████╗
██╔═██╗    ██╔╝  ╚══██╗
██║  ██╗   ██║  █████╔╝
╚═╝  ╚═╝   ╚═╝  ╚════╝
```

## Presentation of k7s tool screens (terminal UI)

### k7s main start view
![](https://raw.githubusercontent.com/bieli/k7s/main/assets/k7s__screenshot_main_screen__v0.4.9.png)

### k7s selected kubernetes resource details view
![](https://raw.githubusercontent.com/bieli/k7s/main/assets/k7s__screenshot_resources_details__v0.4.9.png)

## How to use k7s app.?

- you can use `Tab` key to go into next resource and `Shift+Tab` to move to prev. one
- you can switch between namespaces with keys from `0` to `9` (default start view contains all resources from all namespaces)


## How to run k7s  app.?

### Setup kube config env.

Based on kube config file, internal K8s API client can "speak" with your kubernetes cluster.

```bash
export KUBECONFIG=/home/$USER/.kube/config
```

#### Tip: How to export kubeconfig in different variants of clusters

##### k8s, kind
```bash
kubectl config view --raw > ~/.kube/config
```

##### k0s
```bash
sudo k0s kubeconfig admin > ~/.k0s/config
```

### Install k7s inside your OS

You need to have `cargo` - RUST programming language ecosystem base tool (multiplatform)

Here you can read about [multiplatform support by RUST language](https://xampprocky.github.io/rust-forge/release/platform-support.html)

```bash
sudo apt update
sudo apt install build-essential rustup -y
rustup default stable
```

You need to install `cargo-binstall` tool first (it compiling project from RUST official codes repository!):
- https://github.com/cargo-bins/cargo-binstall
```bash
$ curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
```
- next call like this, and agreed with `yes` in installation step

```bash
$ /home/$USER/.cargo/bin/cargo-binstall k7s
 INFO resolve: Resolving package: 'k7s'
 ...
 WARN The package k7s v0.4.0 will be installed from source (with cargo)
Do you wish to continue? [yes]/no yes
    Updating crates.io index
  Downloaded k7s v0.4.0
  Downloaded 1 crate (325.8KiB) in 0.35s
  Installing k7s v0.4.0
      ...
      Compiling kube v0.88.1
   Compiling k7s v0.4.0
    Finished `release` profile [optimized] target(s) in 1m 13s
  Installing /home/bieli/.cargo/bin/k7s
   Installed package `k7s v0.4.0` (executable `k7s`)
 INFO Cargo finished successfully
 INFO Done in 112.487616926s
```
You can open new terminal window and put directly `/home/$USER/.cargo/bin/k7s` and it works!

### Cleanup Cargo tool after installation

```bash
cp /home/$USER/.cargo/bin/k7s /usr/bin
apt-get purge rustup
```

### Simple by using releases (compiled in CI) binaries
You can look [here on official releases](https://github.com/bieli/k7s/releases) in github for `k7s` project.

There are different architectures binaries, to support you in selection you can use this table as reference.
Below architectures were testet on physical machines:
| architecture | library | prefixed name in release | physical machine / SBC |
|--------------|---------|--------------------------|-----------------------|
| Intel(R) Core(TM) i7 | glibc GLIBC_2.32, GLIBC_2.33, GLIBC_2.34, GLIBC_2.39 | `k7s-x86_64-unknown-linux-gnu` | Intel CORE i7 |
| AMD Ryzen 5 | glibc GLIBC_2.39 | `k7s-x86_64-unknown-linux-gnu` | AMD Ryzen 5 3600 |
| AMD EPYC-Rome Processor  | glibc GLIBC_2.39 | `k7s-x86_64-unknown-linux-gnu` | AMD EPYC 7272 Zen2 |
| ARMv7l / ARMv7 Processor rev 4 (v7l) / Cortex-A7  | musl | `k7s-arm-unknown-linux-gnueabihf` | BananaPI PRO |
| ARMv8-A / aarch64 / Cortex-A53  | musl | `k7s-aarch64-unknown-linux-musl` | Raspberry Pi 3 Model B Rev 1.2 |

#### How to deploy and run `k7s` on your OS
```bash
wget <link to release>
gunzip <release dowloaded TAR.GZ packed file>
tar -xf  <release dowloaded TAR upnacked file>
./k7s
```


### Compile and run - for developers

```bash
cargo run --release
```

or you can build binary

```bash
cargo build --release
```

## Simplistic way to spin up your kubernetes cluster on localhost

You need to install [`kind` tool](https://kind.sigs.k8s.io/) and `docker`.

All you need to do from Linux terminal is:
```bash
kind create cluster
```


## A little bit theory about kubernetes resources - kubernetes resources overview

> A practical reference guide to the most important Kubernetes resource types used in day-to-day DevOps work.

---

### Table of Contents

- [1. Application Management (Workloads)](#1-application-management-workloads)
- [2. Networking & Communication](#2-networking--communication)
- [3. Configuration & Storage](#3-configuration--storage)
- [4. Administration & Access Control](#4-administration--access-control)
- [5. Infrastructure (Cluster Resources)](#5-infrastructure-cluster-resources)

---

### 1. Application Management (Workloads)

> This is where you define **what** runs in the cluster and **how**.

| Resource | Description |
|---|---|
| **Pod** | The smallest deployable unit; one or more containers running together. |
| **Deployment** | The most popular resource; manages Pod replication, enables rolling updates and rollbacks. |
| **ReplicaSet** | A low-level mechanism (usually managed by a Deployment) that ensures a specified number of Pod replicas are running. |
| **StatefulSet** | Used for applications requiring a stable identifier and persistent data storage (e.g. databases). |
| **DaemonSet** | Ensures one copy of a Pod runs on every node in the cluster (e.g. for logging or monitoring). |
| **Job / CronJob** | For one-off tasks or tasks run on a schedule (such as backups). |

---

### 2. Networking & Communication

> Resources that allow applications to talk to each other and to the outside world.

| Resource | Description |
|---|---|
| **Service** | A stable access point (IP/DNS) to a group of Pods. Types: `ClusterIP` (internal), `NodePort` (port on the machine), `LoadBalancer` (external IP from a cloud provider). |
| **Ingress** | Manages incoming HTTP/HTTPS traffic, enabling routing based on domains and URL paths. |
| **NetworkPolicy** | A firewall inside the cluster; defines which Pods are allowed to communicate with each other. |

---

### 3. Configuration & Storage

> Injecting settings and handling persistent data.

| Resource | Description |
|---|---|
| **ConfigMap** | Stores configuration (`.env`, `.yaml` files) as key-value pairs. |
| **Secret** | Used for securely storing sensitive data (passwords, certificates, API keys). |
| **PersistentVolume (PV)** | Cluster-level abstraction representing a piece of storage. |
| **PersistentVolumeClaim (PVC)** | A request for storage by a user; binds to a PV and allows data to be retained after a Pod restarts. |

---

### 4. Administration & Access Control

> Controlling who can do what inside the cluster.

| Resource | Description |
|---|---|
| **Namespace** | Logical isolation of resources within a single cluster (e.g. `dev`, `staging`, `prod`). |
| **ServiceAccount** | An identity for processes running inside Pods. |
| **Role / ClusterRole** | Define a set of permissions (RBAC) — what a given user or service is allowed to read or modify. |
| **RoleBinding / ClusterRoleBinding** | Bind a Role or ClusterRole to a user, group, or ServiceAccount. |

---

### 5. Infrastructure (Cluster Resources)

> Resources describing the physical state of the cluster.

| Resource | Description |
|---|---|
| **Node** | A representation of a server (physical or VM) on which Pods run. |
| **Event** | A log of cluster events (e.g. container startup errors), essential when debugging. |

---

## Resources Map

```
Cluster
├── Namespaces (dev / staging / prod)
│   ├── Workloads
│   │   ├── Deployment -> ReplicaSet -> Pod(s)
│   │   ├── StatefulSet -> Pod(s)
│   │   ├── DaemonSet -> Pod (per Node)
│   │   └── Job / CronJob -> Pod(s)
│   ├── Networking
│   │   ├── Service (ClusterIP / NodePort / LoadBalancer)
│   │   ├── Ingress
│   │   └── NetworkPolicy
│   ├── Config & Storage
│   │   ├── ConfigMap
│   │   ├── Secret
│   │   └── PVC -> PV
│   └── Access Control
│       ├── ServiceAccount
│       ├── Role / RoleBinding
│       └── ClusterRole / ClusterRoleBinding
└── Cluster-level
    └── Nodes -> Events
```

---

*For more details, refer to the [official Kubernetes documentation](https://kubernetes.io/docs/concepts/).*



## TODO list

- [x] update details view to this similar from `kubectl describe ...`
- [ ] add unit tests and fix quality CI in Github Actions
- [ ] add cluster perspective with generic groups of panels + easy switch between
- [ ] add `+ / -` buttons on cluster perspective (prev point.) view to add/remove panels for main start app. view & save settings in user file, when user open again tool, it will be configured to user prefered panels on start screen
- [ ] add `--edit | -e` mode to app. line args. - to enable editable mode (will be very usefull for `CKAD exam`, when you could use this `k7s` tool, when changes are required inside 99% of tasks instead of clicking (time of reaction on changes is one imprtant measure in `CKAD exam`)!)
- [ ] colors schemas like in `btop`, becouse real engineers, who have been using terminal and other geeks, would like to change colors
- [ ] instead of pulling, listining events from kubernetes cluster and propagate on UI panels, depends on events
- [ ] add bash script one liner to easy install binary for everyone
- [ ] add releases with ready to use binaries for multiple hardware architectures of Linux/*NIX OS
- [ ] listining Open Source community, what they want
