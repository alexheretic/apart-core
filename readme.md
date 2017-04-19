Apart Core
==========
Linux util for partition cloning GUIs. ZMQ API for managing partclone jobs. Provides GUI library agnostic control of partclone jobs. This utility is meant for use with a separate GUI client, gtk, qt etc.

## Architecture
```
+-----------------+
| apart presenter |
+-----------------+
     ^
     | zeromq (yaml)
     v
+------------+
| apart core |
+------------+
   + + +
   | | | subprocess
   v v v
+------------------------+
| partclone, pigz, lsblk |
+------------------------+
```

## Starting
Start by invoking the command with a single argument a half-bound ZMQ pair IPC address string ie `ipc:///tmp/apart.ipc`

## Messages
### Clone
To create a new clone job send a `type: clone` message
```yaml
# client -> core
type: clone
source: /dev/sda1 # a valid partition
destination: /mnt/backups/ # some directory absolute path
name: work # name of backup
```
Client will then receive regular update messages on the progress of the clone job
```yaml
# core -> client
type: clone
source: /dev/sda1 # requested partition
destination: /mnt/backups/work.apt.gz # backup image file absolute path
id: uid123 # a uid for this job
complete: 0.0123 # double [0, 1] where 1.0 => it is complete
rate: 9.87GB/min # string describing the rate the job is currently enjoying
start: 2017-04-18T17:39:01Z  # zoned time of start

# [optional fields]
finish: 2017-04-18T17:39:02Z # zoned time of finish, only present when complete = 1
```
To cancel a clone send:
```yaml
# client -> core
type: cancel-clone
id: uid123
```
In the case theres an error completing the clone, ie it's just been cancelled, an error message is sent
```yaml
# core -> client
type: clone-failed
source: /dev/sda1
destination: /mnt/backups/work.apt.gz
id: uid123
error: Cancelled # a reason for the failure
```

### Status
To convey the status of the core itself the presenter/client receives status messages with `type: status`

On startup the following status is sent:
```yaml
# core -> client
type: status
status: started
sources:
- name: sda
  size: 213282455552
  parts:
  - name: sda1 # implies request source /dev/sda1
    size: 181070200832 # size of partition in bytes
    mounted: true # indicates if the partition is currently mounted
    label: Arch # [optional] partition label
  - name: sda2
    size: 32212254720
    mounted: false
```
And before ending the core sends:
```yaml
# core -> client
type: status
status: dying
```

## Dependencies
* zeromq
* partclone
* pigz
