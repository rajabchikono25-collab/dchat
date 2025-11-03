# Multi-Region Validator Infrastructure with Terraform
#
# This Terraform configuration deploys dchat validators across multiple
# geographic regions with proper networking, security, and storage setup.
#
# Supported Providers:
# - AWS (primary)
# - Azure (optional)
# - GCP (optional)
#
# Usage:
#   terraform init
#   terraform plan -var="environment=production"
#   terraform apply -var="environment=production"

terraform {
  required_version = ">= 1.5.0"
  
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.20"
    }
    helm = {
      source  = "hashicorp/helm"
      version = "~> 2.10"
    }
  }
  
  # Remote state backend (recommended)
  backend "s3" {
    bucket         = "dchat-terraform-state"
    key            = "validators/terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "dchat-terraform-locks"
  }
}

# Variables
variable "environment" {
  description = "Environment name (development, staging, production)"
  type        = string
  validation {
    condition     = contains(["development", "staging", "production"], var.environment)
    error_message = "Environment must be development, staging, or production"
  }
}

variable "regions" {
  description = "List of AWS regions to deploy validators"
  type        = list(string)
  default = [
    "us-east-1",      # North America - Virginia
    "us-west-2",      # North America - Oregon
    "eu-west-1",      # Europe - Ireland
    "eu-central-1",   # Europe - Frankfurt
    "ap-southeast-1", # Asia - Singapore
    "ap-northeast-1", # Asia - Tokyo
    "sa-east-1",      # South America - São Paulo
  ]
}

variable "validators_per_region" {
  description = "Number of validator nodes per region"
  type        = number
  default     = 1
}

variable "instance_type" {
  description = "EC2 instance type for validator nodes"
  type        = string
  default     = "c6i.4xlarge" # 16 vCPUs, 32GB RAM
}

variable "volume_size_gb" {
  description = "EBS volume size in GB for validator storage"
  type        = number
  default     = 1000 # 1TB NVMe SSD
}

variable "enable_monitoring" {
  description = "Enable CloudWatch monitoring"
  type        = bool
  default     = true
}

variable "enable_backups" {
  description = "Enable automated EBS snapshots"
  type        = bool
  default     = true
}

# Local values
locals {
  common_tags = {
    Project     = "dchat"
    Environment = var.environment
    ManagedBy   = "terraform"
    Component   = "validator"
  }
  
  validator_count = length(var.regions) * var.validators_per_region
  
  # BFT configuration: 2f+1 where f is maximum Byzantine nodes
  bft_required_signatures = ceil(local.validator_count * 2 / 3)
}

# Data sources
data "aws_availability_zones" "available" {
  for_each = toset(var.regions)
  
  provider = aws.region[each.key]
  state    = "available"
}

# AWS Provider configuration for each region
provider "aws" {
  region = "us-east-1" # Default region
  
  default_tags {
    tags = local.common_tags
  }
}

# Multi-region provider aliases
provider "aws" {
  alias  = "us-east-1"
  region = "us-east-1"
}

provider "aws" {
  alias  = "us-west-2"
  region = "us-west-2"
}

provider "aws" {
  alias  = "eu-west-1"
  region = "eu-west-1"
}

provider "aws" {
  alias  = "eu-central-1"
  region = "eu-central-1"
}

provider "aws" {
  alias  = "ap-southeast-1"
  region = "ap-southeast-1"
}

provider "aws" {
  alias  = "ap-northeast-1"
  region = "ap-northeast-1"
}

provider "aws" {
  alias  = "sa-east-1"
  region = "sa-east-1"
}

# VPC for each region
resource "aws_vpc" "validator_vpc" {
  for_each = toset(var.regions)
  
  provider             = aws.region[each.key]
  cidr_block           = "10.${index(var.regions, each.key)}.0.0/16"
  enable_dns_hostnames = true
  enable_dns_support   = true
  
  tags = merge(local.common_tags, {
    Name   = "dchat-validator-vpc-${each.key}"
    Region = each.key
  })
}

# Internet Gateway
resource "aws_internet_gateway" "validator_igw" {
  for_each = toset(var.regions)
  
  provider = aws.region[each.key]
  vpc_id   = aws_vpc.validator_vpc[each.key].id
  
  tags = merge(local.common_tags, {
    Name = "dchat-validator-igw-${each.key}"
  })
}

# Public subnet for validators
resource "aws_subnet" "validator_subnet" {
  for_each = toset(var.regions)
  
  provider                = aws.region[each.key]
  vpc_id                  = aws_vpc.validator_vpc[each.key].id
  cidr_block              = "10.${index(var.regions, each.key)}.1.0/24"
  availability_zone       = data.aws_availability_zones.available[each.key].names[0]
  map_public_ip_on_launch = true
  
  tags = merge(local.common_tags, {
    Name = "dchat-validator-subnet-${each.key}"
  })
}

# Route table
resource "aws_route_table" "validator_rt" {
  for_each = toset(var.regions)
  
  provider = aws.region[each.key]
  vpc_id   = aws_vpc.validator_vpc[each.key].id
  
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.validator_igw[each.key].id
  }
  
  tags = merge(local.common_tags, {
    Name = "dchat-validator-rt-${each.key}"
  })
}

resource "aws_route_table_association" "validator_rta" {
  for_each = toset(var.regions)
  
  provider       = aws.region[each.key]
  subnet_id      = aws_subnet.validator_subnet[each.key].id
  route_table_id = aws_route_table.validator_rt[each.key].id
}

# Security group for validators
resource "aws_security_group" "validator_sg" {
  for_each = toset(var.regions)
  
  provider    = aws.region[each.key]
  name        = "dchat-validator-sg-${each.key}"
  description = "Security group for dchat validator nodes"
  vpc_id      = aws_vpc.validator_vpc[each.key].id
  
  # P2P networking (TCP)
  ingress {
    description = "P2P TCP"
    from_port   = 7070
    to_port     = 7070
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }
  
  # P2P networking (QUIC/UDP)
  ingress {
    description = "P2P QUIC"
    from_port   = 7070
    to_port     = 7070
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
  }
  
  # RPC endpoint
  ingress {
    description = "RPC endpoint"
    from_port   = 9545
    to_port     = 9545
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }
  
  # Prometheus metrics
  ingress {
    description = "Metrics"
    from_port   = 9090
    to_port     = 9090
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8"] # Internal only
  }
  
  # SSH (restricted to bastion hosts)
  ingress {
    description = "SSH"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8"] # Internal only
  }
  
  # Allow all outbound
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
  
  tags = merge(local.common_tags, {
    Name = "dchat-validator-sg-${each.key}"
  })
}

# IAM role for validator nodes
resource "aws_iam_role" "validator_role" {
  name = "dchat-validator-role-${var.environment}"
  
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })
  
  tags = local.common_tags
}

# IAM policy for CloudWatch and EBS
resource "aws_iam_role_policy" "validator_policy" {
  name = "dchat-validator-policy"
  role = aws_iam_role.validator_role.id
  
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "cloudwatch:PutMetricData",
          "ec2:DescribeVolumes",
          "ec2:CreateSnapshot",
          "ec2:DescribeSnapshots",
          "logs:CreateLogGroup",
          "logs:CreateLogStream",
          "logs:PutLogEvents"
        ]
        Resource = "*"
      }
    ]
  })
}

resource "aws_iam_instance_profile" "validator_profile" {
  name = "dchat-validator-profile-${var.environment}"
  role = aws_iam_role.validator_role.name
}

# Launch template for validator instances
resource "aws_launch_template" "validator_lt" {
  for_each = toset(var.regions)
  
  provider      = aws.region[each.key]
  name          = "dchat-validator-lt-${each.key}"
  image_id      = data.aws_ami.ubuntu[each.key].id
  instance_type = var.instance_type
  
  iam_instance_profile {
    name = aws_iam_instance_profile.validator_profile.name
  }
  
  network_interfaces {
    associate_public_ip_address = true
    security_groups             = [aws_security_group.validator_sg[each.key].id]
    delete_on_termination       = true
  }
  
  block_device_mappings {
    device_name = "/dev/sda1"
    
    ebs {
      volume_size           = var.volume_size_gb
      volume_type           = "gp3" # NVMe SSD
      iops                  = 16000 # Maximum IOPS for gp3
      throughput            = 1000  # Maximum throughput (MB/s)
      delete_on_termination = false # Preserve data
      encrypted             = true
    }
  }
  
  user_data = base64encode(templatefile("${path.module}/user-data.sh", {
    environment = var.environment
    region      = each.key
  }))
  
  tags = merge(local.common_tags, {
    Name = "dchat-validator-lt-${each.key}"
  })
}

# Find latest Ubuntu AMI
data "aws_ami" "ubuntu" {
  for_each = toset(var.regions)
  
  provider    = aws.region[each.key]
  most_recent = true
  owners      = ["099720109477"] # Canonical
  
  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }
  
  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# Auto Scaling Group for validators
resource "aws_autoscaling_group" "validator_asg" {
  for_each = toset(var.regions)
  
  provider            = aws.region[each.key]
  name                = "dchat-validator-asg-${each.key}"
  desired_capacity    = var.validators_per_region
  min_size            = var.validators_per_region
  max_size            = var.validators_per_region * 2
  vpc_zone_identifier = [aws_subnet.validator_subnet[each.key].id]
  health_check_type   = "EC2"
  
  launch_template {
    id      = aws_launch_template.validator_lt[each.key].id
    version = "$Latest"
  }
  
  tag {
    key                 = "Name"
    value               = "dchat-validator-${each.key}"
    propagate_at_launch = true
  }
  
  tag {
    key                 = "Region"
    value               = each.key
    propagate_at_launch = true
  }
  
  dynamic "tag" {
    for_each = local.common_tags
    content {
      key                 = tag.key
      value               = tag.value
      propagate_at_launch = true
    }
  }
}

# Outputs
output "validator_regions" {
  description = "Regions where validators are deployed"
  value       = var.regions
}

output "total_validators" {
  description = "Total number of validator nodes"
  value       = local.validator_count
}

output "bft_required_signatures" {
  description = "Number of signatures required for BFT consensus"
  value       = local.bft_required_signatures
}

output "vpc_ids" {
  description = "VPC IDs by region"
  value       = { for k, v in aws_vpc.validator_vpc : k => v.id }
}

output "security_group_ids" {
  description = "Security group IDs by region"
  value       = { for k, v in aws_security_group.validator_sg : k => v.id }
}
