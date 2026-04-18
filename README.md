![github_tag](https://img.shields.io/github/v/tag/bieli/k7s)
[![Crates.io](https://img.shields.io/crates/v/k7s.svg)](https://crates.io/crates/k7s)

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

## Main view of k7s tool with terminal UI

![](https://raw.githubusercontent.com/bieli/k7s/main/assets/screenshot_03.v0.3.0-alpha.png)


## How to use k7s app.?

- you can use `Tab` key to go into next resource and `Shift+Tab` to move to prev. one
- you can switch between namespaces with keys from `0` to `9` (default start view contains all resources from all namespaces)


## How to run k7s  app.?

### Setup kube config env.

Based on kube config file, internal K8s API client can "speak" with your kubernetes cluster.

```bash
export KUBECONFIG=/home/$USER/.kube/config
```

### Compile and run

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
- [ ] add `--edit | -e` mode to app. line args. - to enable editable mode (will be very usefull for `CKAD exam`, when you could use this `k7s` tool, when changes are required inside 99% of tasks instead of clicking (time of reaction on changes is one imprtant measure in `CKAD exam`)!)
- [ ] colors schemas like in `btop`, becouse real engineers, who have been using terminal and other geeks, would like to change colors
- [ ] instead of pulling, listining events from kubernetes cluster and propagate on UI panels, depends on events
- [ ] add bash script one liner to easy install binary for everyone
- [ ] add releases with ready to use binaries for multiple hardware architectures of Linux/*NIX OS
- [ ] listinin Open Source community, what they want

