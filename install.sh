#!/usr/bin/env bash
set -e

echo "=== Instalador Automatizado VIO (Linux) ==="

# 1. Checagem de privilégios de root
if [ "$EUID" -ne 0 ]; then
  echo "❌ Execute este script com permissões de administrador: sudo bash install.sh"
  exit 1
fi

# 2. Instalação e verificação do Ollama
if ! command -v ollama >/dev/null 2>&1; then
    echo "⬇️ Instalando motor de IA Ollama..."
    curl -fsSL https://ollama.com/install.sh | sh
else
    echo "✅ Ollama já instalado."
fi

# 3. Download do modelo Phi-3
echo "⬇️ Baixando modelo Phi-3 (pode demorar dependendo da sua conexão)..."
ollama pull phi3

# 4. Estrutura de diretórios do sistema
mkdir -p /usr/local/bin
mkdir -p /etc/vio/config_rules

# 5. Download do binário pré-compilado das Releases do GitHub
echo "⬇️ Baixando executável do VIO..."
curl -fsSL -o /usr/local/bin/vio "https://github.com/SEU_USUARIO/SEU_REPOSITORIO/releases/latest/download/vio"
chmod +x /usr/local/bin/vio

# 6. Download das regras padrão JSON
echo "⬇️ Configurando regras de auditoria..."
curl -fsSL -o /etc/vio/config_rules/iniciacao.json "https://raw.githubusercontent.com/SEU_USUARIO/SEU_REPOSITORIO/main/config_rules/iniciacao.json"

echo ""
echo "🎉 Instalação concluída com sucesso!"
echo "O VIO e o modelo Phi-3 já estão prontos para uso em qualquer terminal."