# VIO - VerInOff (Motor de Verificação de Processos Corporativos)

O **VIO** é uma ferramenta de linha de comando (CLI) desenvolvida em Rust para auditoria e validação de conformidade de processos corporativos (baseada no processo **SpinOff**). Ele inspeciona automaticamente diretórios de projetos de software, extrai conteúdo de artefatos estruturados (Word `.docx`, Excel `.xlsx`, diagramas UML/Astah `.asta`/`.uml`) e utiliza modelos locais de linguagem (SLM via Ollama, como o `phi3`) para analisar se os requisitos e boas práticas foram cumpridos de forma 100% offline.

---

## Sumário

* [Visão Geral e Arquitetura](#visão-geral-e-arquitetura)
* [Todas as Funcionalidades](#todas-as-funcionalidades)
* [Instalação e Pré-requisitos](#instalação-e-pré-requisitos)
* [Passo a Passo de Uso](#passo-a-passo-de-uso)
* [Referência de Linha de Comando (CLI)](#referência-de-linha-de-comando-cli)
* [Como Montar o JSON de Configuração](#como-montar-o-json-de-configuração)
* [Padrão de Resposta da IA e Proteção contra Loop Infinito](#padrão-de-resposta-da-ia-e-proteção-contra-loop-infinito)
* [Estrutura do Banco de Dados SQLite e Logs](#estrutura-do-banco-de-dados-sqlite-e-logs)

---

## Visão Geral e Arquitetura

O VIO atua como um auditor automatizado em pipelines ou ambientes locais:

1. **Varredura Estrutural Inteligente:** Identifica a pasta raiz do projeto, substitui dinamicamente o identificador `{SIGLA}` e reconhece variações de acentuação, caixa alta/baixa, singular/plural e múltiplos artefatos por pasta (ex.: várias Histórias de Usuário `UC01..UCn`, múltiplas Atas de Reunião datadas ou múltiplos Protótipos).
2. **Extração Nativa de Conteúdo:**
   * **Word (`.docx`):** Extrai textos preservando quebras de parágrafo e estruturas de tabelas.
   * **Excel (`.xlsx`):** Extrai abas específicas ou todas as abas relevantes (`"*"`), ignorando linhas vazias e abas auxiliares de cálculo (`AUX`).
   * **Astah / UML (`.asta` / `.uml`):** Descompacta e decodifica nativamente o fluxo `EntityStore` de arquivos binários `.asta` do Astah, extraindo pacotes, diagramas, atores, casos de uso, classes e descrições UML preenchidas.
3. **Auditoria com IA (Ollama):** Submete o conteúdo extraído junto com prompts regulatórios calibrados para o modelo local (`phi3`), processando as saídas em tempo real (*streaming*).
4. **Prevenção de Plágio/Desvio de Escopo:** Analisa documentos de Visão para fixar a "identidade do projeto". Auditorias subsequentes recebem esse contexto.
5. **Detecção de Arquivos Estranhos:** Rastreador recursivo que denuncia artefatos soltos não pertencentes ao processo SpinOff (ignorando arquivos temporários `~$` do Office, `.git`, templates `.dotx`/`.xltx` e artefatos legítimos de outras fases).
6. **Persistência e Dashboards:** Armazena resultados em SQLite (atualizando execuções repetidas da mesma fase sem duplicar linhas) e gera relatórios em texto, CSV e painéis HTML interativos com gráficos (Chart.js).

---

## Todas as Funcionalidades

### 1. Inicialização Automática do Motor de IA

O VIO verifica via socket TCP se a API do Ollama está ativa na porta configurada (ex: `11434`). Caso esteja desligada, o binário localiza o executável do Ollama (no `PATH` ou em `%LOCALAPPDATA%\Programs\Ollama\ollama.exe`), executa `ollama serve` em segundo plano e aguarda a inicialização antes de processar as requisições.

### 2. Validação Flexível de Existência vs. Conteúdo

* **Apenas Existência (`apenas_existencia: true`):** Checa deterministicamente se o artefato existe na pasta correta da estrutura do projeto sem consumir tempo de processamento da IA.
* **Inspeção Semântica Profunda:** Extrai textos de documentos, abas de planilhas e elementos UML do Astah e aplica o prompt avaliativo via modelo local.

### 3. Memória Dinâmica e Detecção de Identidade

* Identifica regras associadas ao documento de "Visão" e gera um resumo do sistema.
* Injeta o resumo em todas as regras seguintes, mantendo o contexto do projeto avaliado.
* Mantém o histórico das avaliações anteriores em memória durante a execução.

### 4. Varredura de "Arquivos Estranhos"

Percorre o diretório do projeto e compara todos os arquivos encontrados com os artefatos conhecidos pelo processo SpinOff. Caso encontre arquivos estranhos à estrutura, marca o item como `Desconhecido` e gera a sugestão de remoção ou remanejamento.

### 5. Cálculo Automático de Indicador de Aderência (IAP SpinOff)

Calcula a taxa de aderência idêntica à planilha oficial de Checklist de Verificação do SpinOff:

$$\text{Aderência (\%)} = \left(\frac{\text{Qtd. Sim}}{\text{Qtd. Válidos (Sim + Parcialmente + Não)}}\right) \times 100$$

*(Registros categorizados como "Não se aplica" são excluídos do denominador).*

### 6. Relatórios e Dashboards

* **Terminal:** Interface formatada com destaque para status, desvios e sugestões.
* **CSV:** Arquivo tabular com codificação UTF-8 com BOM (`\u{FEFF}`), pronto para abertura direta no Microsoft Excel.
* **HTML Elegante:** Dashboard visual completo com métrica percentual destacada, tabela com bordas coloridas conforme severidade e gráfico *doughnut* gerado via Chart.js.

### 7. Atualização Automática Integrada (`--update`)

* Executa `ollama pull phi3` para atualizar os pesos do modelo.
* Faz consulta direta via API do GitHub Releases para auto-atualizar o próprio executável (`vio.exe`) sem reinstalação manual.

---

## Instalação e Pré-requisitos

### Instalação Automatizada (Windows)

Execute o arquivo gerado pelo Inno Setup (`Instalador_VIO.exe`). O instalador:

1. Instala o executável e as regras das 4 fases (`iniciacao.json`, `elaboracao.json`, `construcao.json`, `transicao.json`) em `%ProgramFiles%\VIO`.
2. Inclui o diretório no `PATH` do sistema operacional.
3. Checa a presença do **Ollama**; se ausente, faz o download do `OllamaSetup.exe` e executa instalação silenciosa.
4. Faz o pull automático do modelo padrão: `ollama pull phi3`.

### Instalação Automatizada / Manual (Linux)

Execute o script `install.sh` como root:
```bash
sudo bash install.sh
```

---

## Passo a Passo de Uso

### Passo 1: Organizar a Estrutura do Projeto

O VIO usa o nome do diretório alvo como a `{SIGLA}` do projeto, seguindo o Guia de Mapeamento de Produtos por Processo do SpinOff:

```text
MEUPROJETO/
├── 1.Requisitos/
│   ├── MEUPROJETO - Visão.docx
│   ├── MEUPROJETO - Glossário.docx (opcional)
│   ├── Casos de Uso/
│   │   └── MEUPROJETO - UC01 - História de Usuário.docx
│   └── Prototipo/
│       └── MEUPROJETO - Protótipo.png
├── 2.Analise e Design/
│   └── MEUPROJETO - Modelo de Análise e Design.asta
├── 3.Implementacao/
├── 4.Teste/
│   └── MEUPROJETO - Roteiro de Testes.xlsx
├── 5.Implantacao/
│   ├── MEUPROJETO - Guia de Implantação.docx
│   └── MEUPROJETO - Manual do Usuário.docx (opcional)
└── 6.Gerenciamento de Projeto/
    ├── MEUPROJETO - Planejamento e Controle do Projeto.xlsx
    └── Atas/
        └── MEUPROJETO - Ata de Reunião.docx
```

### Passo 2: Executar a Auditoria

Abra o terminal e aponte para a pasta do projeto (opcionalmente especificando a fase):

```bash
vio ./MEUPROJETO iniciacao.json
```

Ou execute todas as fases (`iniciacao`, `elaboracao`, `construcao`, `transicao`) em sequência:

```bash
vio ./MEUPROJETO
```

### Passo 3: Gerar Dashboards e Exportações

Para extrair um relatório visual com gráficos e uma planilha de resultados consolidados da auditoria já executada:

```bash
vio --gerar-relatorio --projeto MEUPROJETO --html --exportar-csv --pasta-saida ./relatorios_auditoria
```

---

## Referência de Linha de Comando (CLI)

```bash
vio [CAMINHO_PROJETO] [ARQUIVO_CONFIG] [OPÇÕES]
```

### Argumentos Posicionais

* `caminho_projeto`: Diretório do projeto a ser auditado (padrão: `.`).
* `arquivo_config`: Nome do arquivo JSON específico dentro de `config_rules/` (opcional; se omitido, executa as fases `iniciacao.json`, `elaboracao.json`, `construcao.json` e `transicao.json`).

### Opções e Flags

| Flag | Descrição | Exemplo de Uso |
| --- | --- | --- |
| `--gerar-relatorio` | Gera relatório consolidado lendo os dados salvos no SQLite | `vio --gerar-relatorio` |
| `--html` | Exporta relatório em painel HTML com gráficos Chart.js | `vio --gerar-relatorio --html` |
| `--exportar-csv` | Exporta relatório em planilha CSV estruturada | `vio --gerar-relatorio --exportar-csv` |
| `--pasta-saida <DIR>` | Define pasta customizada de saída para CSV/HTML | `vio --gerar-relatorio --html --pasta-saida ./docs` |
| `--projeto <NOME>` | Filtra os registros do banco pelo nome da sigla | `vio --mostrar-banco --projeto MEUPROJ` |
| `--fase <JSON>` | Filtra os registros do banco pelo arquivo de regras | `vio --gerar-relatorio --fase iniciacao.json` |
| `--id_regra <TEXTO>` | Filtra registros que contenham o termo no ID da regra | `vio --mostrar-banco --id_regra "Backlog"` |
| `--mostrar-banco` | Imprime os dados brutos gravados no banco SQLite | `vio --mostrar-banco` |
| `--limpar-banco` | Limpa as tabelas de auditoria e identidades salvas | `vio --limpar-banco` |
| `--toggle-quiet` | Alterna modo silencioso (oculta streaming do LLM) | `vio --toggle-quiet` |
| `--update` | Atualiza o modelo de IA e busca release mais recente | `vio --update` |

---

## Como Montar o JSON de Configuração

Os arquivos JSON devem residir na pasta `config_rules/`.

### Estrutura Completa dos Campos

* **`endpoint_slm`** *(string, obrigatório)*: URL da API do Ollama. Ex: `"http://localhost:11434/api/generate"`.
* **`modelo_slm`** *(string, obrigatório)*: Modelo a ser invocado. Ex: `"phi3"`.
* **`pastas_ignoradas_extracao`** *(lista de strings, opcional)*: Pastas que a busca recursiva de "arquivos estranhos" deve pular. Ex: `["3.Implementacao", ".git", ".vscode"]`.
* **`regras_conformidade`** *(lista de objetos)*:
  * **`id_regra`** *(string)*: Identificador legível ou pergunta da regra.
  * **`tipo_arquivo`** *(string)*: Formato de extração. Aceita `"word"`, `"excel"`, `"astah"` ou `"uml"`.
  * **`pasta_esperada`** *(string)*: Subdiretório onde o artefato deve estar localizado.
  * **`nome_arquivo_base`** *(string)*: Nome base do arquivo sem extensão. O curinga `{SIGLA}` é resolvido automaticamente pelo nome da pasta do projeto.
  * **`obrigatorio`** *(boolean)*: Se `true` e o arquivo não for encontrado, marca como reprovado (`"Não"`). Se `false`, marca como `"Não se aplica"`.
  * **`apenas_existencia`** *(boolean, opcional)*: Se `true`, valida apenas a existência do arquivo, sem chamar o modelo de IA.
  * **`aba_planilha`** *(string, opcional)*: Nome da(s) aba(s) da planilha caso o tipo seja `"excel"` (separadas por vírgula, ou `"*"` para todas as abas úteis).
  * **`prompt_ia`** *(string)*: Prompt de auditoria enviado ao SLM.

---

## Padrão de Resposta da IA e Proteção contra Loop Infinito

O motor do VIO realiza até **3 tentativas** (`MAX_TENTATIVAS_IA = 3`) para obter uma resposta que satisfaça a Expressão Regular compilada no código Rust (com fallback automático na última tentativa para impedir loops infinitos):

```rust
(?is)(?:Análise|Analise|ANALISAÇÃO).*?:\s*(.*?)\s*Desvio.*?:\s*(.*?)\s*(?:Sugestão|Sugestao|SUGÉSTAO|SUGÉSTIMO).*?:\s*(.*?)\s*Status Qualitativo.*?:\s*[*_\[\]]*\s*(Não se aplica|Nao se aplica|N/A|NA|Sim|Não|Nao|Parcialmente)
```

### Template Recomendado do `prompt_ia`

Todo prompt semântico deve terminar com as seguintes diretrizes de formatação:

```text
RESPONDA ESTRITAMENTE NESTE FORMATO:
Análise: [Breve análise em até 2 frases]
Desvio: [Descreva o problema ou 'Nenhum']
Sugestão: [Sugestão de melhoria ou 'Nenhuma']
Status Qualitativo: [Sim / Não / Parcialmente / Não se aplica]
```

---

## Estrutura do Banco de Dados SQLite e Logs

O VIO armazena os dados no diretório de dados locais do sistema operacional (`%LOCALAPPDATA%\VIO\auditorias.db` no Windows).

### Tabelas Internas

#### `identidade_projeto`

Armazena o perfil semântico sintetizado do sistema com base na regra de Visão:

* `projeto` (TEXT PRIMARY KEY): Sigla do projeto.
* `resumo_identidade` (TEXT NOT NULL): Resumo produzido pelo SLM.

#### `historico_auditoria`

Mantém o registro histórico de cada item avaliado:

* `id` (INTEGER PRIMARY KEY AUTOINCREMENT)
* `projeto` (TEXT NOT NULL)
* `fase_json` (TEXT NOT NULL): Nome do arquivo de regra aplicado.
* `id_regra` (TEXT NOT NULL): Identificador da regra.
* `arquivo` (TEXT NOT NULL): Nome resolvido do arquivo verificado.
* `status` (TEXT NOT NULL): `Sim`, `Não`, `Parcialmente` ou `Não se aplica`.
* `desvio` (TEXT): Descrição do problema ou incongruência detectada.
* `sugestao` (TEXT): Plano de mitigação ou ajuste sugerido pela IA.
* `data_auditoria` (DATETIME): Data e hora da auditoria.

### Arquivo de Configuração do Usuário (`settings.json`)

Localizado em `%LOCALAPPDATA%\VIO\settings.json`:

```json
{
  "quiet_mode": false
}
```

### Arquivos de Logs de Falhas

Falhas de rede, problemas de comunicação com o Ollama ou erros na leitura de arquivos são registrados em:

```text
%LOCALAPPDATA%\VIO\logs\auditoria_erros.log
```
