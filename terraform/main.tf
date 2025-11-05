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
  default     = "postgresql://rajab:sLiaFpvhwzPSBEnvBoc2jg@absurd-auroch-17923.j77.cockroachlabs.cloud:26257/dchat?sslmode=verify-full"
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
