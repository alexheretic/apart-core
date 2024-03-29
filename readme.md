Apart Core
==========
Linux util for partition cloning GUIs. ZMQ API for managing partclone jobs. Provides GUI library agnostic control of partclone jobs. This utility is meant for use with a separate GUI client, gtk, qt etc.

GUI logic, 'apart presenters', start the core as a running command and communicate with it via ZMQ yaml string messages. Apart Core is then responsible for providing all the info needed for the presenter, and can be instructed to perform partition cloning & restoring.

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
+------------------------------------------+
| partclone, lsblk, zstdmt, pigz, lz4, cat |
+------------------------------------------+
```

## Starting
Start by invoking the command with a single argument a half-bound ZMQ pair IPC address string ie `ipc:///tmp/apart.ipc`.
Apart Core will connect to the ZMQ socket address, ie it is expected that the address is already be bound by the client.

Something like the following in python with the `zmq` lib
```python
# example python client/presenter
zmq_ctx = zmq.Context()
zmq_socket = zmq_ctx.socket(zmq.PAIR)
zmq_address = 'ipc:///tmp/apart.ipc'
zmq_socket.bind(zmq_address)

# start apart-core referring it to our ZMQ address
subprocess.Popen(['apart-core', zmq_address])

# receive the initial status message
initial_status_msg = zmq_socket.recv_string()

# send messages to clone/restore partitions
...

# ask apart-core to shutdown
zmq_socket.send_string('type: kill-request')   
```

## Messages
### Clone
To start creating a partition image from a partition send a `type: clone` YAML message into the ZMQ socket
```yaml
# client -> core
type: clone
source: /dev/sda1  # a valid partition
destination: /mnt/backups/  # some directory absolute path
name: work  # name of backup

# [optional fields]
compression: gz # assumes `gz` if absent, see status message `compression_options`
```
Client will then receive regular update messages on the progress of the clone job
```yaml
# core -> client
type: clone
source: /dev/sda1  # requested partition
destination: /mnt/backups/work-2017-05-03T1020.apt.dd.gz  # backup image file absolute path
id: 8db93abe  # a uid for this job
complete: 0.0123  # double [0, 1] where 1.0 => it is complete
syncing: false  # indicates the transfer is complete the final syncing process has started
start: 2017-04-18T17:39:01Z  # utc time of start

# [optional fields]
source_uuid: 32b35cf2-052b-4a31-8f3b-c3e4bfeaa689  # partition UUID if available
rate: 9.87GB/min  # string describing the rate the job is currently enjoying, present when available
estimated_finish: 2017-04-18T17:40:03Z  # zoned time of estimated finish, present when available

# present when job has finished successfully
finish: 2017-04-18T17:40:02Z  # utc time of finish
image_size: 536766054400  # size of created image file (bytes)
```
To cancel a clone send:
```yaml
# client -> core
type: cancel-clone
id: 8db93abe
```
In the case an error has occurred, ie it's been cancelled, a `type: clone-failed` is sent
```yaml
# core -> client
type: clone-failed
source: /dev/sda1
destination: /mnt/backups/work-2017-05-03T1020.apt.dd.gz
id: 8db93abe
start: 2017-04-18T17:39:01Z
finish: 2017-04-18T17:39:03Z  # utc time of failure
error: Cancelled  # a reason for the failure
```
Successfully created images can be deleted by sending:
```yaml
# client -> core
type: delete-clone
file: /mnt/backups/work-2017-05-03T1020.apt.dd.gz
```
A successful delete returns the response:
```yaml
# core -> client
type: deleted-clone
file: /mnt/backups/work-2017-05-03T1020.apt.dd.gz
```
Failure will return:
```yaml
# core -> client
type: delete-clone-failed
file: /mnt/backups/work-2017-05-03T1020.apt.dd.gz
error: No such file
```

### Restore
Apart core can restore partitions using images it has previously created.
To start restoring a partition from an image send a `type: restore` message.
Take note this action will destroy the current state of the partition, so GUIs should warn the user of this.
```yaml
# client -> core
type: restore
# image file created using the clone functionality
source: /mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz  
destination: /dev/sda1  # partition to restore
```
Similarly to a clone the client will then receive regular update messages
```yaml
# core -> client
type: restore
source: /mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz  
destination: /dev/sda1
id: d4323700  # a uid for this job
complete: 0.0123  # double [0, 1] where 1.0 => it is complete
syncing: false  # indicates the transfer is complete the final syncing process has started
start: 2017-04-18T17:39:01Z  # utc time of start

# [optional fields]
rate: 9.87GB/min  # string describing the rate the job is currently enjoying, present when available
estimated_finish: 2017-04-18T17:40:03Z  # zoned time of estimated finish, present when available

# present when job has finished successfully
finish: 2017-04-18T17:40:02Z  # utc time of finish
```
To cancel a restore send:
```yaml
# client -> core
type: cancel-restore
id: d4323700
```
In the case an error has occurred, ie it's been cancelled, a `type: restore-failed` is sent
```yaml
# core -> client
type: restore-failed
source: /mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz  
destination: /dev/sda1
id: d4323700
start: 2017-04-18T17:39:01Z
finish: 2017-04-18T17:39:03Z
error: Cancelled
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
  - name: sda1  # implies request source /dev/sda1
    size: 181070200832  # size of partition in bytes
    mounted: true  # indicates if the partition is currently mounted
    label: Arch  # [optional] partition label
    fstype: ext4  # [optional] file system
    uuid: c699a42a-d91b-4b1d-9cc7-ddd6b40a08a2  # [optional] unique id
  - name: sda2
    size: 32212254720
    mounted: false
compression_options:
- gz  # default, always available; provided by `pigz` as a required dependency
- uncompressed # always available
- lz4  # available if `lz4` is installed
- zst  # available if `zstdmt` is installed
```
To get an updated status message for whatever reason send:
```yaml
# client -> core
type: status-request
```
core will reply with a status message similar to the above but with `status: running`
and other info up to date if applicable

Before exiting the core will send:
```yaml
# core -> client
type: status
status: dying
```

### Shutdown
To shutdown the core simply send:
```yaml
# client -> core
type: kill-request
```
Core will cancel all jobs and shutdown (sending the dying status above)

## Dependencies
* zeromq >= 4.1
* util-linux >= 2.28.2
* partclone
* pigz
* lz4 *(optional: adds compression option)*
* zst *(optional: adds compression option)*
