# lrad

[![Build Status](https://travis-ci.org/sameer/lrad.svg?branch=master)](https://travis-ci.org/sameer/lrad)
[![Coverage Status](https://coveralls.io/repos/github/sameer/lrad/badge.svg?branch=master)](https://coveralls.io/github/sameer/lrad?branch=master)

![logo](lrad.svg)

An update framework for applications running on hobbyist single-board-computers (CS5285 Final Project)

## TODOs

- [ ] lrad-cli
  - [ ] Init
    - [x] Create config file
    - [ ] Interactvie "wizard"
  - [ ] Push
    - [ ] Transform Git repo
      - [x] Clone bare repository
      - [ ] Unpack objects
      - [x] update-server-info
      - [ ] Size constraints
    - [ ] Add to IPFS
      - [x] Local IPFS API server
        - [ ] Convert to actix-web client
      - [ ] Remote IPFS API server
        - [ ] SSH tunnel
        - [ ] Are there other ways to securely connect?
    - [ ] (FUTURE) Use ipld-git, it is not mature right now but is the ideal candidate
    - [ ] Put DNS link record
      - [x] Cloudflare
      - [ ] AWS Route 53
      - [ ] Namecheap
      - [ ] Google DNS
- [ ] lrad-daemon
  - [ ] Init (how?)
  - [ ] Update
    - [ ] Linux
      - [ ] Systemd service
    - [ ] Windows (not for now)
    - [ ] Update stages (finite state machine)
  - [x] DNS txt record polling

## Motivation

### What is IoT

Over the course of electronic computer system history, there has been a consistent trend of systems becoming more compact, more powerful, and more common. Today, there are devices ranging from electric scooters to baby monitors to prosthetics that can all be connected to the internet. This movement has come to be known as the Internet of Things (IoT). While the applications are many, IoT has only really reached the common consumer in the past five years through smart home devices like Amazon's Alexa or Nest's Thermostat.

#### Security Concerns

While IoT promises to bring the next wave of inter-connectivity in our lives, there are several barriers that hinder it from becoming the be-all and end-all of twenty-first century computing. One involves security concerns. If these systems are not properly protected, they will introduce more risk than value; it makes no sense to install a smart lock on your front door if someone can easily exploit a vulnerability in it and lock you out of your home. The attack surface for an IoT device tends to be much larger than that of the traditional device it replaces.

#### Security through Secure Updates

It is inevitable that someone will discover an IoT device with a zero-day security vulnerability. Currently, high-profile news of a breach has little effect manufacturer profits, it is expensive to have independent security audits done, and humans are just not perfect. With that in mind, it is important that there be a process to update these devices with bug fixes. In this project, I intend to explore current state of the art processes and build upon them in order to design and implement a process ideal for hobbyist single-board computers (SBCs) like those released by the Rasberry Pi Foundation or BeagleBoard.org Foundation. This process

### Objectives

This project aims to design and implement a process for remote SBC updates. The process will be demoed using an in-production system used by the [Vanderbilt Design Studio](https://github.com/vanderbilt-design-studio/).

Implement the Process in the Rust Programming Language

### Project Plan

#### Design Requirements

- Secure: Minimize the attack surface and make resistant to attacks. Does not try to re-invent the wheel.

- Remote: Works remotely over the internet -- having technicians on-site just to manually install updates would be unreasonable.

- Low-Footprint : Does not hinder normal operation of the system. Namely, it should not compromise the system or make it unavailable.

- Prioritized : Not all updates need to run immediately, so a timeframe for the update can be specified.

- Realtime : Devices immediately (within a few seconds) discover that a new update is available.

- Decentralized : Cannot hinder the update process by DoS, fraudulent DMCA, DNS hijacking, or other means.

## Implementation

The process will be implemented using the Rust, a systems programming language that focuses on safety without sacrificing performance. It will be open-source, as all projects regarding security should be.

## Timeline

- [x] Identify state of the art processes currently in place

- [x] Select libraries to use for project

- [x] Draft system architecture

- [x] Review system architecture

- [x] Begin implementation

- [ ] Revise design as needed

- [ ] Begin testing on production system

- [ ] Finish implementation and testing
