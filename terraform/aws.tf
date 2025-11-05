# AWS Provider Configuration
provider "aws" {
  alias  = "ohio"
  region = "us-east-2"
}

provider "aws" {
  alias  = "saopaulo"
  region = "sa-east-1"
}

provider "aws" {
  alias  = "singapore"
  region = "ap-southeast-1"
}

provider "aws" {
  alias  = "stockholm"
  region = "eu-north-1"
}

# Security Group for AWS Validators
resource "aws_security_group" "validator" {
  provider    = aws.ohio
  name        = "dchat-validator-sg"
  description = "Security group for dchat validators"

  # SSH
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "SSH access"
  }

  # P2P Networking
  ingress {
    from_port   = 7070
    to_port     = 7070
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "dchat P2P TCP"
  }

  ingress {
    from_port   = 7070
    to_port     = 7070
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "dchat P2P UDP/QUIC"
  }

  # Health Check
  ingress {
    from_port   = 8080
    to_port     = 8080
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Health check endpoint"
  }

  # Prometheus Metrics
  ingress {
    from_port   = 9090
    to_port     = 9090
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Prometheus metrics"
  }

  # Tendermint P2P
  ingress {
    from_port   = 26656
    to_port     = 26656
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "Tendermint P2P"
  }

  # Redis (internal only - restrict in production)
  ingress {
    from_port   = 6379
    to_port     = 6379
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
    description = "Redis (internal)"
  }

  # MinIO API
  ingress {
    from_port   = 9000
    to_port     = 9000
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
    description = "MinIO API (internal)"
  }

  # MinIO Console
  ingress {
    from_port   = 9001
    to_port     = 9001
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "MinIO Console (restrict in production)"
  }

  # TiKV PD
  ingress {
    from_port   = 2379
    to_port     = 2380
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
    description = "TiKV PD (internal)"
  }

  # TiKV
  ingress {
    from_port   = 20160
    to_port     = 20160
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
    description = "TiKV (internal)"
  }

  # Outbound
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "All outbound traffic"
  }

  tags = {
    Name        = "dchat-validator-sg"
    Project     = "dchat"
    Environment = "production"
  }
}

# Copy security group to other AWS regions
resource "aws_security_group" "validator_saopaulo" {
  provider    = aws.saopaulo
  name        = "dchat-validator-sg"
  description = "Security group for dchat validators"

  # Same rules as above
  dynamic "ingress" {
    for_each = aws_security_group.validator.ingress
    content {
      from_port   = ingress.value.from_port
      to_port     = ingress.value.to_port
      protocol    = ingress.value.protocol
      cidr_blocks = ingress.value.cidr_blocks
      description = ingress.value.description
    }
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name        = "dchat-validator-sg"
    Project     = "dchat"
    Environment = "production"
  }
}

resource "aws_security_group" "validator_singapore" {
  provider    = aws.singapore
  name        = "dchat-validator-sg"
  description = "Security group for dchat validators"

  dynamic "ingress" {
    for_each = aws_security_group.validator.ingress
    content {
      from_port   = ingress.value.from_port
      to_port     = ingress.value.to_port
      protocol    = ingress.value.protocol
      cidr_blocks = ingress.value.cidr_blocks
      description = ingress.value.description
    }
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name        = "dchat-validator-sg"
    Project     = "dchat"
    Environment = "production"
  }
}

resource "aws_security_group" "validator_stockholm" {
  provider    = aws.stockholm
  name        = "dchat-validator-sg"
  description = "Security group for dchat validators"

  dynamic "ingress" {
    for_each = aws_security_group.validator.ingress
    content {
      from_port   = ingress.value.from_port
      to_port     = ingress.value.to_port
      protocol    = ingress.value.protocol
      cidr_blocks = ingress.value.cidr_blocks
      description = ingress.value.description
    }
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name        = "dchat-validator-sg"
    Project     = "dchat"
    Environment = "production"
  }
}

# SSH Key Pair for AWS
resource "aws_key_pair" "dchat_ohio" {
  provider   = aws.ohio
  key_name   = "dchat-validator-key"
  public_key = var.ssh_public_key

  tags = {
    Name    = "dchat-validator-key"
    Project = "dchat"
  }
}

resource "aws_key_pair" "dchat_saopaulo" {
  provider   = aws.saopaulo
  key_name   = "dchat-validator-key"
  public_key = var.ssh_public_key

  tags = {
    Name    = "dchat-validator-key"
    Project = "dchat"
  }
}

resource "aws_key_pair" "dchat_singapore" {
  provider   = aws.singapore
  key_name   = "dchat-validator-key"
  public_key = var.ssh_public_key

  tags = {
    Name    = "dchat-validator-key"
    Project = "dchat"
  }
}

resource "aws_key_pair" "dchat_stockholm" {
  provider   = aws.stockholm
  key_name   = "dchat-validator-key"
  public_key = var.ssh_public_key

  tags = {
    Name    = "dchat-validator-key"
    Project = "dchat"
  }
}

# AWS Instances - Ubuntu 22.04 LTS
data "aws_ami" "ubuntu_ohio" {
  provider    = aws.ohio
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

data "aws_ami" "ubuntu_saopaulo" {
  provider    = aws.saopaulo
  most_recent = true
  owners      = ["099720109477"]

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

data "aws_ami" "ubuntu_singapore" {
  provider    = aws.singapore
  most_recent = true
  owners      = ["099720109477"]

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

data "aws_ami" "ubuntu_stockholm" {
  provider    = aws.stockholm
  most_recent = true
  owners      = ["099720109477"]

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# EC2 Instances
resource "aws_instance" "validator_ohio" {
  provider      = aws.ohio
  ami           = data.aws_ami.ubuntu_ohio.id
  instance_type = "t3.xlarge" # 4 vCPU, 16 GB RAM
  key_name      = aws_key_pair.dchat_ohio.key_name

  vpc_security_group_ids = [aws_security_group.validator.id]

  root_block_device {
    volume_size = 200 # 200 GB for data storage
    volume_type = "gp3"
  }

  user_data = templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-ohio"
    region                  = "ohio"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  })

  tags = {
    Name        = "dchat-validator-ohio"
    Project     = "dchat"
    Region      = "ohio"
    Environment = "production"
  }
}

resource "aws_instance" "validator_saopaulo" {
  provider      = aws.saopaulo
  ami           = data.aws_ami.ubuntu_saopaulo.id
  instance_type = "t3.xlarge"
  key_name      = aws_key_pair.dchat_saopaulo.key_name

  vpc_security_group_ids = [aws_security_group.validator_saopaulo.id]

  root_block_device {
    volume_size = 200
    volume_type = "gp3"
  }

  user_data = templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-saopaulo"
    region                  = "saopaulo"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  })

  tags = {
    Name        = "dchat-validator-saopaulo"
    Project     = "dchat"
    Region      = "saopaulo"
    Environment = "production"
  }
}

resource "aws_instance" "validator_singapore" {
  provider      = aws.singapore
  ami           = data.aws_ami.ubuntu_singapore.id
  instance_type = "t3.xlarge"
  key_name      = aws_key_pair.dchat_singapore.key_name

  vpc_security_group_ids = [aws_security_group.validator_singapore.id]

  root_block_device {
    volume_size = 200
    volume_type = "gp3"
  }

  user_data = templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-singapore"
    region                  = "singapore"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  })

  tags = {
    Name        = "dchat-validator-singapore"
    Project     = "dchat"
    Region      = "singapore"
    Environment = "production"
  }
}

resource "aws_instance" "validator_stockholm" {
  provider      = aws.stockholm
  ami           = data.aws_ami.ubuntu_stockholm.id
  instance_type = "t3.xlarge"
  key_name      = aws_key_pair.dchat_stockholm.key_name

  vpc_security_group_ids = [aws_security_group.validator_stockholm.id]

  root_block_device {
    volume_size = 200
    volume_type = "gp3"
  }

  user_data = templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-stockholm"
    region                  = "stockholm"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  })

  tags = {
    Name        = "dchat-validator-stockholm"
    Project     = "dchat"
    Region      = "stockholm"
    Environment = "production"
  }
}
