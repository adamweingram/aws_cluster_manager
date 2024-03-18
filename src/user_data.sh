#!/usr/bin/env bash

# Configure Script
PROJECT_TAG="liuyao-msccl-experiments-sc24"
HOSTS_FILE_PATH="/home/ec2-user/hostfile"
IP_FILE_PATH="/home/ec2-user/ip-addresses.txt"

# Install dependencies
yum -y install jq      # For Amazon Linux 2, installs 'jq' package for JSON processing

# # Create root SSH key if it doesn't already exist
# # NOTE: Doesn't help across many nodes as each will be different!
# if [ ! -f /root/.ssh/id_ed25519 ]; then
#     ssh-keygen -t ed25519 -a 100 -N "" -f /root/.ssh/id_ed25519
#     cat /root/.ssh/id_ed25519.pub >> /root/.ssh/authorized_keys
# fi

# Get information about the instances using the AWS "instance metadata" functionality
# REGION=$(curl -s http://169.254.169.254/latest/meta-data/placement/availability-zone | sed 's/[a-z]$//')
AZ="$(ec2-metadata -z | awk '{ print $2 }')" 
echo "[INFO] Got availability zone: ${AZ}"

# Get the instance ID of _this_ particular instance
# INSTANCE_ID=$(curl -s http://169.254.169.254/latest/meta-data/instance-id)
INSTANCE_ID="$(ec2-metadata -i | awk '{ print $2 }')"
echo "[INFO] Got own instance ID: ${INSTANCE_ID}"

# Get Hostnames
INSTANCE_PRIVATE_HOSTNAME=$(ec2-metadata --local-hostname | awk '{ print $2 }')
INSTANCE_PUBLIC_HOSTNAME=$(ec2-metadata --public-hostname | awk '{ print $2 }')

# Get the VPC ID
VPC_ID=$(aws ec2 describe-instances --instance-ids "${INSTANCE_ID}" --filters "Name=availability-zone,Values=${AZ}" --query "Reservations[].Instances[].VpcId" --output text)
echo "[INFO] Got VPC ID: ${VPC_ID}"

# Get the instance type of _this_particular instance
INSTANCE_TYPE="$(ec2-metadata -t | awk '{ print $2 }')"

# Get info about this instance type
INSTANCE_NUM_CORES=$(aws ec2 describe-instance-types --instance-types "${INSTANCE_TYPE}" --query 'InstanceTypes[].VCpuInfo.DefaultVCpus' | jq -r .[])
INSTANCE_NUM_PHYS_CORES=$(aws ec2 describe-instance-types --instance-types "${INSTANCE_TYPE}" --query 'InstanceTypes[].VCpuInfo.DefaultCores' | jq -r .[])
INSTANCE_MEM_MIB=$(aws ec2 describe-instance-types --instance-types "${INSTANCE_TYPE}" --query 'InstanceTypes[].MemoryInfo.SizeInMiB' | jq -r .[])
SLOTS_PER_HOST="${INSTANCE_NUM_PHYS_CORES}"  # 2 hyperthreads per core when on most x86_64 instances, so we'll just use the number of physical cores
echo "[INFO] Using information about ${INSTANCE_TYPE}: ${INSTANCE_NUM_CORES} cores, ${INSTANCE_MEM_MIB} MiB memory, ${SLOTS_PER_HOST} slots per host (for MPI; prob. 2 hyperthreads per core so less than total cores)"

# Get a list of the instances
# CLUSTER_INSTANCE_IPS="$(aws ec2 describe-instances --filters "Name=vpc-id,Values=$VPC_ID" --region "${REGION}" --query "Reservations[].Instances[].PrivateIpAddress" --output json | jq 'sort_by(.)' | jq -r .[])"
# CLUSTER_PUBLIC_IPS="$(aws ec2 describe-instances --filters "Name=vpc-id,Values=$VPC_ID" --region "${REGION}" --query "Reservations[].Instances[].PublicIpAddress" --output json | jq 'sort_by(.)' | jq -r .[])"
CLUSTER_INSTANCE_IPS=$(aws ec2 describe-instances --filters "Name=tag:project,Values=${PROJECT_TAG}" --query "Reservations[].Instances[].PrivateIpAddress" --output json | jq 'sort_by(.)' | jq -r .[])
CLUSTER_PUBLIC_IPS=$(aws ec2 describe-instances --filters "Name=tag:project,Values=${PROJECT_TAG}" --query "Reservations[].Instances[].PublicIpAddress" --output json | jq 'sort_by(.)' | jq -r .[])
echo "[INFO] Got cluster private IPs: ${CLUSTER_INSTANCE_IPS}"
echo "[INFO] Got cluster public IPs: ${CLUSTER_PUBLIC_IPS}"

# Clear the hosts file if it already exists (don't want duplicates)
truncate -s 0 "${HOSTS_FILE_PATH}"

# Update /etc/hosts and create a hosts file for MPI
COUNT=1
echo "${CLUSTER_INSTANCE_IPS}" | while read -r IP; do
    # Create the hostname by combining "worker" with the numerical ID assigned
    HOST_NAME="worker${COUNT}"
    echo "[INFO] Instance ${IP} will be named ${HOST_NAME}"

    # Insert into /etc/hosts
    echo "${IP}   ${HOST_NAME}" >> /etc/hosts

    # Insert into the MPI hosts file
    echo "${HOST_NAME} slots=${SLOTS_PER_HOST}" >> "${HOSTS_FILE_PATH}"

    # # Create a file with just a list of the IPs
    # echo "${IP}" >> "${IP_FILE_PATH}"

    # Increment the counter
    COUNT=$(( COUNT + 1 ))
done

# Create a file with just a list of the IPs
echo "${CLUSTER_PUBLIC_IPS}" >> "${IP_FILE_PATH}"

# Fix permissions on generated files
chown ec2-user:ec2-user "${IP_FILE_PATH}"
chown ec2-user:ec2-user "${HOSTS_FILE_PATH}"

# Upgrade version of custom nccl testing harness
pushd /home/ec2-user/deps/nccl_harness || exit
git pull
cargo build --release
popd || exit
