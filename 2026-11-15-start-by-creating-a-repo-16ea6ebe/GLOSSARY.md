# Network Trace

A desktop tool that keeps tracing the route to one or more hosts, recording latency and packet loss at every hop over time, so connection problems can be pinned down and handed to an ISP as evidence.

## Language

### Monitoring

**Target**:
A host (hostname or IP address) whose route is being monitored.
_Avoid_: Destination, endpoint, host

**Session**:
One continuous period of monitoring a single Target, from start to stop. It is the unit that is saved, reopened and exported, and several Sessions can run at the same time.
_Avoid_: Run, trace, job

**Hop**:
One position along the path to a Target, numbered by its distance from this machine. A Hop keeps its number even when a different address starts answering at that position. The final Hop is the Target itself.
_Avoid_: Node, router

**Route**:
The ordered list of addresses answering at each Hop at a given moment.
_Avoid_: Path, traceroute

**Route Change**:
A moment in a Session when the Route differs from the one before it.
_Avoid_: Reroute, flap

**Probe**:
A single ICMP echo request sent to one Hop, which either gets a reply with a round-trip time or is counted as lost.
_Avoid_: Ping, packet

**Sample**:
One round of Probes sent to every Hop at a single moment.
_Avoid_: Round, iteration, tick

**Gap**:
A stretch of a Session with no Samples because nothing was being sent, for example while the machine slept. A Gap is not loss.
_Avoid_: Outage, downtime

**Time Window**:
The part of a Session being looked at or exported, either a preset (last 30 minutes, last 24 hours, full Session and so on) or a custom range.
_Avoid_: Range, period, focus

### Sharing

**Note**:
Optional free text attached to a Session describing the problem being investigated, shown at the top of its Report.
_Avoid_: Comment, description

**Report**:
A human-readable export of a Session, made for sending to an ISP. It can be viewed in a browser or printed to PDF.
_Avoid_: Summary, printout

**Raw Export**:
A CSV export of every Probe result in a Session.
_Avoid_: Dump, log
