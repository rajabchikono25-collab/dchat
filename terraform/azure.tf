# Azure Provider Configuration
provider "azurerm" {
  features {}
}

# Resource Groups
resource "azurerm_resource_group" "india" {
  name     = "dchat-india-rg"
  location = "Central India"

  tags = {
    Project     = "dchat"
    Region      = "india"
    Environment = "production"
  }
}

resource "azurerm_resource_group" "southafrica" {
  name     = "dchat-southafrica-rg"
  location = "South Africa North"

  tags = {
    Project     = "dchat"
    Region      = "southafrica"
    Environment = "production"
  }
}

resource "azurerm_resource_group" "uae" {
  name     = "dchat-uae-rg"
  location = "UAE North"

  tags = {
    Project     = "dchat"
    Region      = "uae"
    Environment = "production"
  }
}

# Virtual Networks
resource "azurerm_virtual_network" "india" {
  name                = "dchat-india-vnet"
  address_space       = ["10.1.0.0/16"]
  location            = azurerm_resource_group.india.location
  resource_group_name = azurerm_resource_group.india.name

  tags = {
    Project = "dchat"
    Region  = "india"
  }
}

resource "azurerm_virtual_network" "southafrica" {
  name                = "dchat-southafrica-vnet"
  address_space       = ["10.2.0.0/16"]
  location            = azurerm_resource_group.southafrica.location
  resource_group_name = azurerm_resource_group.southafrica.name

  tags = {
    Project = "dchat"
    Region  = "southafrica"
  }
}

resource "azurerm_virtual_network" "uae" {
  name                = "dchat-uae-vnet"
  address_space       = ["10.3.0.0/16"]
  location            = azurerm_resource_group.uae.location
  resource_group_name = azurerm_resource_group.uae.name

  tags = {
    Project = "dchat"
    Region  = "uae"
  }
}

# Subnets
resource "azurerm_subnet" "india" {
  name                 = "dchat-india-subnet"
  resource_group_name  = azurerm_resource_group.india.name
  virtual_network_name = azurerm_virtual_network.india.name
  address_prefixes     = ["10.1.1.0/24"]
}

resource "azurerm_subnet" "southafrica" {
  name                 = "dchat-southafrica-subnet"
  resource_group_name  = azurerm_resource_group.southafrica.name
  virtual_network_name = azurerm_virtual_network.southafrica.name
  address_prefixes     = ["10.2.1.0/24"]
}

resource "azurerm_subnet" "uae" {
  name                 = "dchat-uae-subnet"
  resource_group_name  = azurerm_resource_group.uae.name
  virtual_network_name = azurerm_virtual_network.uae.name
  address_prefixes     = ["10.3.1.0/24"]
}

# Network Security Groups
resource "azurerm_network_security_group" "validator" {
  name                = "dchat-validator-nsg"
  location            = azurerm_resource_group.india.location
  resource_group_name = azurerm_resource_group.india.name

  # SSH
  security_rule {
    name                       = "SSH"
    priority                   = 100
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_range     = "22"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  # P2P TCP
  security_rule {
    name                       = "P2P-TCP"
    priority                   = 110
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_range     = "7070"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  # P2P UDP/QUIC
  security_rule {
    name                       = "P2P-UDP"
    priority                   = 120
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Udp"
    source_port_range          = "*"
    destination_port_range     = "7070"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  # Health Check
  security_rule {
    name                       = "Health"
    priority                   = 130
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_range     = "8080"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  # Metrics
  security_rule {
    name                       = "Metrics"
    priority                   = 140
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_range     = "9090"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  # Tendermint
  security_rule {
    name                       = "Tendermint"
    priority                   = 150
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_range     = "26656"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  # MinIO Console
  security_rule {
    name                       = "MinIO-Console"
    priority                   = 160
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_range     = "9001"
    source_address_prefix      = "*"
    destination_address_prefix = "*"
  }

  tags = {
    Project = "dchat"
  }
}

# Copy NSG to other regions
resource "azurerm_network_security_group" "validator_southafrica" {
  name                = "dchat-validator-nsg"
  location            = azurerm_resource_group.southafrica.location
  resource_group_name = azurerm_resource_group.southafrica.name

  dynamic "security_rule" {
    for_each = azurerm_network_security_group.validator.security_rule
    content {
      name                       = security_rule.value.name
      priority                   = security_rule.value.priority
      direction                  = security_rule.value.direction
      access                     = security_rule.value.access
      protocol                   = security_rule.value.protocol
      source_port_range          = security_rule.value.source_port_range
      destination_port_range     = security_rule.value.destination_port_range
      source_address_prefix      = security_rule.value.source_address_prefix
      destination_address_prefix = security_rule.value.destination_address_prefix
    }
  }

  tags = {
    Project = "dchat"
  }
}

resource "azurerm_network_security_group" "validator_uae" {
  name                = "dchat-validator-nsg"
  location            = azurerm_resource_group.uae.location
  resource_group_name = azurerm_resource_group.uae.name

  dynamic "security_rule" {
    for_each = azurerm_network_security_group.validator.security_rule
    content {
      name                       = security_rule.value.name
      priority                   = security_rule.value.priority
      direction                  = security_rule.value.direction
      access                     = security_rule.value.access
      protocol                   = security_rule.value.protocol
      source_port_range          = security_rule.value.source_port_range
      destination_port_range     = security_rule.value.destination_port_range
      source_address_prefix      = security_rule.value.source_address_prefix
      destination_address_prefix = security_rule.value.destination_address_prefix
    }
  }

  tags = {
    Project = "dchat"
  }
}

# Public IPs
resource "azurerm_public_ip" "india" {
  name                = "dchat-india-ip"
  location            = azurerm_resource_group.india.location
  resource_group_name = azurerm_resource_group.india.name
  allocation_method   = "Static"
  sku                 = "Standard"

  tags = {
    Project = "dchat"
    Region  = "india"
  }
}

resource "azurerm_public_ip" "southafrica" {
  name                = "dchat-southafrica-ip"
  location            = azurerm_resource_group.southafrica.location
  resource_group_name = azurerm_resource_group.southafrica.name
  allocation_method   = "Static"
  sku                 = "Standard"

  tags = {
    Project = "dchat"
    Region  = "southafrica"
  }
}

resource "azurerm_public_ip" "uae" {
  name                = "dchat-uae-ip"
  location            = azurerm_resource_group.uae.location
  resource_group_name = azurerm_resource_group.uae.name
  allocation_method   = "Static"
  sku                 = "Standard"

  tags = {
    Project = "dchat"
    Region  = "uae"
  }
}

# Network Interfaces
resource "azurerm_network_interface" "india" {
  name                = "dchat-india-nic"
  location            = azurerm_resource_group.india.location
  resource_group_name = azurerm_resource_group.india.name

  ip_configuration {
    name                          = "internal"
    subnet_id                     = azurerm_subnet.india.id
    private_ip_address_allocation = "Dynamic"
    public_ip_address_id          = azurerm_public_ip.india.id
  }

  tags = {
    Project = "dchat"
    Region  = "india"
  }
}

resource "azurerm_network_interface" "southafrica" {
  name                = "dchat-southafrica-nic"
  location            = azurerm_resource_group.southafrica.location
  resource_group_name = azurerm_resource_group.southafrica.name

  ip_configuration {
    name                          = "internal"
    subnet_id                     = azurerm_subnet.southafrica.id
    private_ip_address_allocation = "Dynamic"
    public_ip_address_id          = azurerm_public_ip.southafrica.id
  }

  tags = {
    Project = "dchat"
    Region  = "southafrica"
  }
}

resource "azurerm_network_interface" "uae" {
  name                = "dchat-uae-nic"
  location            = azurerm_resource_group.uae.location
  resource_group_name = azurerm_resource_group.uae.name

  ip_configuration {
    name                          = "internal"
    subnet_id                     = azurerm_subnet.uae.id
    private_ip_address_allocation = "Dynamic"
    public_ip_address_id          = azurerm_public_ip.uae.id
  }

  tags = {
    Project = "dchat"
    Region  = "uae"
  }
}

# Associate NSGs with NICs
resource "azurerm_network_interface_security_group_association" "india" {
  network_interface_id      = azurerm_network_interface.india.id
  network_security_group_id = azurerm_network_security_group.validator.id
}

resource "azurerm_network_interface_security_group_association" "southafrica" {
  network_interface_id      = azurerm_network_interface.southafrica.id
  network_security_group_id = azurerm_network_security_group.validator_southafrica.id
}

resource "azurerm_network_interface_security_group_association" "uae" {
  network_interface_id      = azurerm_network_interface.uae.id
  network_security_group_id = azurerm_network_security_group.validator_uae.id
}

# Virtual Machines
resource "azurerm_linux_virtual_machine" "validator_india" {
  name                = "dchat-validator-india"
  location            = azurerm_resource_group.india.location
  resource_group_name = azurerm_resource_group.india.name
  size                = "Standard_D4s_v5" # 4 vCPU, 16 GB RAM
  admin_username      = "dchat"

  network_interface_ids = [
    azurerm_network_interface.india.id,
  ]

  admin_ssh_key {
    username   = "dchat"
    public_key = var.ssh_public_key
  }

  os_disk {
    caching              = "ReadWrite"
    storage_account_type = "Premium_LRS"
    disk_size_gb         = 200
  }

  source_image_reference {
    publisher = "Canonical"
    offer     = "0001-com-ubuntu-server-jammy"
    sku       = "22_04-lts-gen2"
    version   = "latest"
  }

  custom_data = base64encode(templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-india"
    region                  = "india"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  }))

  tags = {
    Name        = "dchat-validator-india"
    Project     = "dchat"
    Region      = "india"
    Environment = "production"
  }
}

resource "azurerm_linux_virtual_machine" "validator_southafrica" {
  name                = "dchat-validator-southafrica"
  location            = azurerm_resource_group.southafrica.location
  resource_group_name = azurerm_resource_group.southafrica.name
  size                = "Standard_D4s_v5"
  admin_username      = "dchat"

  network_interface_ids = [
    azurerm_network_interface.southafrica.id,
  ]

  admin_ssh_key {
    username   = "dchat"
    public_key = var.ssh_public_key
  }

  os_disk {
    caching              = "ReadWrite"
    storage_account_type = "Premium_LRS"
    disk_size_gb         = 200
  }

  source_image_reference {
    publisher = "Canonical"
    offer     = "0001-com-ubuntu-server-jammy"
    sku       = "22_04-lts-gen2"
    version   = "latest"
  }

  custom_data = base64encode(templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-southafrica"
    region                  = "southafrica"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  }))

  tags = {
    Name        = "dchat-validator-southafrica"
    Project     = "dchat"
    Region      = "southafrica"
    Environment = "production"
  }
}

resource "azurerm_linux_virtual_machine" "validator_uae" {
  name                = "dchat-validator-uae"
  location            = azurerm_resource_group.uae.location
  resource_group_name = azurerm_resource_group.uae.name
  size                = "Standard_D4s_v5"
  admin_username      = "dchat"

  network_interface_ids = [
    azurerm_network_interface.uae.id,
  ]

  admin_ssh_key {
    username   = "dchat"
    public_key = var.ssh_public_key
  }

  os_disk {
    caching              = "ReadWrite"
    storage_account_type = "Premium_LRS"
    disk_size_gb         = 200
  }

  source_image_reference {
    publisher = "Canonical"
    offer     = "0001-com-ubuntu-server-jammy"
    sku       = "22_04-lts-gen2"
    version   = "latest"
  }

  custom_data = base64encode(templatefile("${path.module}/user-data.sh", {
    hostname                = "validator1-uae"
    region                  = "uae"
    minio_root_password     = var.minio_root_password
    redis_password          = var.redis_password
    cockroachdb_connection  = var.cockroachdb_connection_string
  }))

  tags = {
    Name        = "dchat-validator-uae"
    Project     = "dchat"
    Region      = "uae"
    Environment = "production"
  }
}
