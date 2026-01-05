# dchat Foundation Infrastructure - Terraform
# Deploys 7 regional validators with complete storage stack

terraform {
  required_version = ">= 1.5"
  
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 3.0"
    }
  }
}

# Variables
variable "ssh_public_key" {
  description = "SSH public key for server access"
  type        = string
}

variable "minio_root_password" {
  description = "MinIO root password"
  type        = string
  sensitive   = true
}

variable "redis_password" {
  description = "Redis password for production"
  type        = string
  sensitive   = true
  default     = ""
}

variable "cockroachdb_connection_string" {
  description = "CockroachDB Cloud connection string"
  type        = string
  sensitive   = true
  default     = ""

  validation {
    condition     = length(trim(var.cockroachdb_connection_string)) > 0
    error_message = "cockroachdb_connection_string must be set (do not hardcode credentials in Terraform files)."
  }
}

variable "ssh_cidrs" {
  description = "CIDR blocks allowed to SSH to validators (restrict in production)"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "health_cidrs" {
  description = "CIDR blocks allowed to access health endpoint (8080)"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "metrics_cidrs" {
  description = "CIDR blocks allowed to access Prometheus metrics (9090)"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "minio_console_cidrs" {
  description = "CIDR blocks allowed to access MinIO console (9001)"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "domain" {
  description = "Base domain for validators"
  type        = string
  default     = "schikuno.top"
}

# Outputs
output "validator_ips" {
  description = "IP addresses of all validators"
  value = {
    ohio        = aws_instance.validator_ohio.public_ip
    saopaulo    = aws_instance.validator_saopaulo.public_ip
    singapore   = aws_instance.validator_singapore.public_ip
    stockholm   = aws_instance.validator_stockholm.public_ip
    india       = azurerm_linux_virtual_machine.validator_india.public_ip_address
    southafrica = azurerm_linux_virtual_machine.validator_southafrica.public_ip_address
    uae         = azurerm_linux_virtual_machine.validator_uae.public_ip_address
  }
}

output "ssh_commands" {
  description = "SSH commands to connect to validators"
  value = {
    ohio        = "ssh dchat@validator1-ohio.${var.domain}"
    saopaulo    = "ssh dchat@validator1-saopaulo.${var.domain}"
    singapore   = "ssh dchat@validator1-singapore.${var.domain}"
    stockholm   = "ssh dchat@validator1-stockholm.${var.domain}"
    india       = "ssh dchat@validator1-india.${var.domain}"
    southafrica = "ssh dchat@validator1-southafrica.${var.domain}"
    uae         = "ssh dchat@validator1-uae.${var.domain}"
  }
}
