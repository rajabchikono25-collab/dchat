# dchat Infrastructure Automation

This directory contains Terraform and Ansible configurations for deploying dchat Foundation infrastructure across 7 global regions.

## Directory Structure

```
.
├── terraform/              # Terraform infrastructure as code
│   ├── main.tf            # Main configuration and outputs
│   ├── aws.tf             # AWS resources (4 regions)
│   ├── azure.tf           # Azure resources (3 regions)
│   ├── user-data.sh       # Bootstrap script for servers
│   └── terraform.tfvars.example  # Variable examples
│
└── ansible/               # Ansible configuration management
    ├── playbook.yml       # Main deployment playbook
    ├── inventory.ini      # Server inventory
    └── templates/
        └── config.toml.j2 # dchat config template
```

---

## Terraform Deployment

### Prerequisites

1. **Install Terraform** (v1.5+):
   ```bash
   # Windows (Chocolatey)
   choco install terraform
   
   # macOS (Homebrew)
   brew install terraform
   
   # Linux
   wget https://releases.hashicorp.com/terraform/1.6.0/terraform_1.6.0_linux_amd64.zip
   unzip terraform_1.6.0_linux_amd64.zip
   sudo mv terraform /usr/local/bin/
   ```

2. **Configure Cloud Credentials**:

   **AWS**:
   ```bash
   # Install AWS CLI
   pip install awscli
   
   # Configure credentials
   aws configure
   # Enter: Access Key ID, Secret Access Key, Default region (us-east-2), Default output (json)
   ```

   **Azure**:
   ```bash
   # Install Azure CLI
   curl -sL https://aka.ms/InstallAzureCLIDeb | sudo bash
   
   # Login
   az login
   ```

3. **Generate SSH Key**:
   ```bash
   ssh-keygen -t ed25519 -f ~/.ssh/dchat-validator -C "dchat-validator"
   ```

### Deployment Steps

1. **Initialize Terraform**:
   ```bash
   cd terraform
   terraform init
   ```

2. **Create Variables File**:
   ```bash
   cp terraform.tfvars.example terraform.tfvars
   nano terraform.tfvars
   ```

   Update with your values:
   ```hcl
   ssh_public_key = "ssh-ed25519 AAAAC3Nza... dchat-validator"
   minio_root_password = "YourSecurePassword123!"
   redis_password = "YourRedisPassword123"  # Optional
   ```

3. **Plan Deployment**:
   ```bash
   terraform plan
   ```

4. **Deploy Infrastructure**:
   ```bash
   terraform apply
   # Review changes and type 'yes' to confirm
   ```

5. **Get Outputs**:
   ```bash
   terraform output validator_ips
   terraform output ssh_commands
   ```

### What Terraform Deploys

- **7 Virtual Machines** (4 AWS + 3 Azure):
  - Instance Type: t3.xlarge / Standard_D4s_v5 (4 vCPU, 16 GB RAM)
  - Storage: 200 GB SSD
  - OS: Ubuntu 22.04 LTS

- **Network Configuration**:
  - Security Groups / NSGs with firewall rules
  - Public IP addresses
  - VPC/VNet and subnets (Azure)

- **Automated Bootstrap** (via user-data):
  - Docker installation
  - Docker Compose installation
  - Redis deployment (port 6379)
  - MinIO deployment (ports 9000, 9001)
  - MinIO bucket creation
  - Firewall configuration (UFW)
  - dchat configuration file

### Terraform Commands

```bash
# View current state
terraform show

# List resources
terraform state list

# Destroy infrastructure
terraform destroy

# Update single region (e.g., Ohio)
terraform apply -target=aws_instance.validator_ohio
```

---

## Ansible Deployment

### Prerequisites

1. **Install Ansible**:
   ```bash
   # Windows (WSL required)
   pip install ansible
   
   # macOS
   brew install ansible
   
   # Linux
   sudo apt install ansible  # Ubuntu/Debian
   sudo dnf install ansible  # Fedora/RHEL
   ```

2. **Configure Inventory**:
   ```bash
   cd ansible
   nano inventory.ini
   ```

   Update IP addresses with your actual server IPs.

3. **Set Environment Variables**:
   ```bash
   export MINIO_ROOT_PASSWORD="YourSecurePassword123!"
   export REDIS_PASSWORD="YourRedisPassword123"  # Optional
   export COCKROACHDB_CONNECTION="postgresql://..."
   ```

### Deployment Steps

1. **Test Connectivity**:
   ```bash
   ansible validators -i inventory.ini -m ping
   ```

2. **Run Playbook (Dry Run)**:
   ```bash
   ansible-playbook -i inventory.ini playbook.yml --check
   ```

3. **Deploy to All Validators**:
   ```bash
   ansible-playbook -i inventory.ini playbook.yml
   ```

4. **Deploy to Specific Region**:
   ```bash
   # Deploy to AWS validators only
   ansible-playbook -i inventory.ini playbook.yml --limit aws_validators
   
   # Deploy to single validator
   ansible-playbook -i inventory.ini playbook.yml --limit validator1-ohio.schikuno.top
   ```

### What Ansible Deploys

- System updates and package installation
- Docker and Docker Compose
- Redis container with persistence
- MinIO container with bucket creation
- dchat configuration file
- Firewall rules (UFW)
- Status monitoring script

### Ansible Commands

```bash
# Run specific tasks
ansible-playbook -i inventory.ini playbook.yml --tags docker
ansible-playbook -i inventory.ini playbook.yml --tags storage

# Verbose output
ansible-playbook -i inventory.ini playbook.yml -vvv

# Run as different user
ansible-playbook -i inventory.ini playbook.yml -u ubuntu

# Skip specific tasks
ansible-playbook -i inventory.ini playbook.yml --skip-tags firewall
```

---

## Post-Deployment

### Verify Deployment

1. **Check DNS Resolution**:
   ```bash
   # From project root
   .\verify-dns.ps1
   ```

2. **SSH into Validators**:
   ```bash
   ssh dchat@validator1-ohio.schikuno.top
   ./status.sh
   ```

3. **Check Storage Services**:
   ```bash
   # Redis
   docker exec redis redis-cli ping
   
   # MinIO
   curl http://localhost:9000/minio/health/live
   
   # MinIO Console
   # Open in browser: http://validator1-ohio.schikuno.top:9001
   # Login: dchat_admin / <your-password>
   ```

### Update DNS Records

After Terraform deployment, update your DNS records to point to the new IP addresses:

```bash
# Get all IPs
terraform output validator_ips

# Update DNS (example for Cloudflare)
# validator1-ohio.schikuno.top → <OHIO-IP>
# validator1-saopaulo.schikuno.top → <SAOPAULO-IP>
# ... etc
```

### Deploy dchat Validator Binary

Once the dchat validator binary is built:

```bash
# Copy binary to all validators
for host in $(ansible validators -i ansible/inventory.ini --list-hosts | grep -v hosts); do
  scp target/release/dchat-validator dchat@$host:/home/dchat/dchat/
done

# Enable and start service
ansible validators -i ansible/inventory.ini -m systemd -a "name=dchat-validator state=started enabled=yes" --become
```

---

## Cost Estimates

### AWS (4 validators)
- **t3.xlarge instances**: ~$0.1664/hour × 4 = $0.6656/hour
- **Storage (200 GB GP3)**: ~$0.08/GB/month × 4 = $64/month
- **Data Transfer**: Variable (estimate $50-100/month)
- **Total**: ~$600-700/month for AWS validators

### Azure (3 validators)
- **Standard_D4s_v5**: ~$0.192/hour × 3 = $0.576/hour
- **Premium SSD (200 GB)**: ~$30/month × 3 = $90/month
- **Data Transfer**: Variable (estimate $40-80/month)
- **Total**: ~$550-650/month for Azure validators

### Combined Total: ~$1,150-1,350/month

### Cost Optimization
- Use Reserved Instances (AWS) or Reserved VM Instances (Azure) for 30-50% savings
- Scale down instance types during testing
- Use Spot Instances for non-critical validators

---

## Troubleshooting

### Terraform Issues

**Issue**: "Error creating instance"
```bash
# Check credentials
aws sts get-caller-identity
az account show

# Check quotas
aws service-quotas list-service-quotas --service-code ec2
```

**Issue**: "Resource already exists"
```bash
# Import existing resource
terraform import aws_instance.validator_ohio i-1234567890abcdef0
```

### Ansible Issues

**Issue**: "Host unreachable"
```bash
# Check SSH connectivity
ssh -i ~/.ssh/dchat-validator dchat@validator1-ohio.schikuno.top

# Check SSH key permissions
chmod 600 ~/.ssh/dchat-validator
```

**Issue**: "Permission denied"
```bash
# Ensure user has sudo access
ansible validators -i inventory.ini -m shell -a "sudo -l" -u dchat
```

### Storage Issues

**Issue**: Redis not starting
```bash
# Check logs
docker logs redis

# Restart
docker restart redis
```

**Issue**: MinIO buckets not created
```bash
# Manual creation
docker exec -it minio /bin/sh
mc alias set dchat http://localhost:9000 dchat_admin <password>
mc mb dchat/dchat-testnet
```

---

## Security Recommendations

1. **Restrict SSH Access**:
   ```hcl
   # In terraform/aws.tf, change:
   cidr_blocks = ["YOUR.IP.ADDRESS/32"]  # Instead of 0.0.0.0/0
   ```

2. **Enable Redis Authentication**:
   ```bash
   # Set redis_password variable
   redis_password = "StrongPassword123!"
   ```

3. **Use HTTPS for MinIO**:
   - Configure SSL certificates (Let's Encrypt)
   - Update `use_ssl = true` in config

4. **Rotate Credentials Regularly**:
   ```bash
   # Update MinIO password
   docker exec -it minio mc admin user remove dchat dchat_admin
   docker exec -it minio mc admin user add dchat dchat_admin NewPassword123!
   ```

5. **Enable Firewall Logging**:
   ```bash
   sudo ufw logging on
   sudo tail -f /var/log/ufw.log
   ```

---

## Next Steps

1. **Build dchat Binary**: Compile the Rust validator binary
2. **Deploy Binary**: Use Ansible to deploy to all validators
3. **Generate Validator Keys**: Create unique keys for each validator
4. **Configure Bootstrap Peers**: Update config with peer IDs
5. **Start Validators**: Enable and start dchat-validator service
6. **Monitor**: Set up Prometheus/Grafana for monitoring
7. **Deploy TiKV**: Set up TiKV cluster for distributed storage

---

## Support & Documentation

- **Full Deployment Guide**: `Foundation-servers/DEPLOYMENT_CHECKLIST.md`
- **Storage Setup**: `STORAGE_QUICK_REF.txt`
- **Architecture**: `ARCHITECTURE.md`
- **DNS Setup**: `Foundation-servers/DNS_CONFIGURED.md`

For issues, check the logs:
```bash
# Terraform
terraform show
terraform state list

# Ansible
ansible-playbook -i inventory.ini playbook.yml -vvv

# Server logs
ssh dchat@validator1-ohio.schikuno.top
journalctl -u dchat-validator -f
docker logs redis
docker logs minio
```
