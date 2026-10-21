# VIO - VerInOff (Motor de Verificação de Processos Corporativos)

O **VIO** é uma ferramenta de linha de comando (CLI) desenvolvida em Rust para auditoria e validação de conformidade de processos corporativos. Ele inspeciona automaticamente diretórios de projetos de software, extrai conteúdo de artefatos estruturados (Word, Excel, diagramas UML) e utiliza modelos locais de linguagem (SLM via Ollama) para analisar se os requisitos e boas práticas foram cumpridos.

---

## Sumário

* [Visão Geral e Arquitetura](https://www.google.com/search?q=%23vis%C3%A3o-geral-e-arquitetura)
* [Todas as Funcionalidades](https://www.google.com/search?q=%23todas-as-funcionalidades)
* [Instalação e Pré-requisitos](https://www.google.com/search?q=%23instala%C3%A7%C3%A3o-e-pr%C3%A9-requisitos)
* [Passo a Passo de Uso](https://www.google.com/search?q=%23passo-a-passo-de-uso)
* [Referência de Linha de Comando (CLI)](https://www.google.com/search?q=%23refer%C3%AAncia-de-linha-de-comando-cli)
* [Como Montar o JSON de Configuração](https://www.google.com/search?q=%23como-montar-o-json-de-configura%C3%A7%C3%A3o)
* [Atenção Crítica: O Loop Infinito da IA](https://www.google.com/search?q=%23aten%C3%A7%C3%A3o-cr%C3%ADtica-o-loop-infinito-da-ia)
* [Estrutura do Banco de Dados SQLite e Logs](https://www.google.com/search?q=%23estrutura-do-banco-de-dados-sqlite-e-logs)

---

## Visão Geral e Arquitetura

O VIO atua como um auditor automatizado em pipelines ou ambientes locais:

1. **Varredura Estrutural:** Identifica a pasta raiz do projeto e substitui dinamicamente o identificador `{SIGLA}` nos nomes dos arquivos esperados.
2. **Extração de Conteúdo:** Lê dados de arquivos `.docx` (Word), planilhas `.xlsx` (Excel com suporte a abas específicas) e arquivos de modelagem textual/UML.
3. **Auditoria com IA (Ollama):** Submete o conteúdo extraído junto com prompts regulatórios para o modelo local (ex: `phi3`), processando as saídas em tempo real (*streaming*).
4. **Prevenção de Plágio/Desvio de Escopo:** Analisa documentos de Visão para fixar a "identidade do projeto". Auditorias subsequentes são confrontadas contra esse contexto.
5. **Detecção de Arquivos Estranhos:** Rastreador recursivo que denuncia artefatos soltos em pastas não mapeadas.
6. **Persistência e Dashboards:** Armazena resultados em SQLite e gera relatórios em texto, CSV e painéis HTML interativos com gráficos (Chart.js).

---

## Todas as Funcionalidades

### 1. Inicialização Automática do Motor de IA

O VIO verifica via socket TCP se a API do Ollama está ativa na porta configurada (ex: `11434`). Caso esteja desligada, o binário executa `ollama serve` em segundo plano e aguarda 5 segundos antes de processar as requisições.

### 2. Validação Flexível de Existência vs. Conteúdo

* **Apenas Existência (`apenas_existencia: true`):** Checa se o arquivo existe no caminho esperado sem consumir tempo de processamento da IA (ideal para protótipos, binários ou mídias).
* **Inspeção Semântica Profunda:** Extrai textos e abas de planilhas e aplica o prompt avaliativo via modelo local.

### 3. Memória Dinâmica e Detecção de Identidade

* Identifica regras marcadas como "Visão" e gera um resumo do sistema.
* Injeta o resumo em todas as regras seguintes: `IDENTIDADE: '<resumo>'. Reprove se fugir do contexto.`, impedindo que documentos reaproveitados de outros projetos passem despercebidos.
* Mantém o histórico das avaliações anteriores em memória para retroalimentar a análise em lote.

### 4. Varredura de "Arquivos Estranhos"

Percorre o diretório do projeto e compara todos os arquivos encontrados com a lista de arquivos conhecidos pelas regras. Caso encontre arquivos não cadastrados (ignorando diretórios configurados como `.git` ou pastas de compilação), marca o item como `Desconhecido` e gera a sugestão de remoção ou remanejamento.

### 5. Cálculo Automático de Indicador de Aderência

Calcula a taxa de aderência global e por ciclo:


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

1. Instala o executável em `%ProgramFiles%\VIO`.
2. Inclui o diretório no `PATH` do sistema operacional.
3. Checa a presença do **Ollama**; se ausente, faz o download do `OllamaSetup.exe` e executa instalação silenciosa.
4. Faz o pull automático do modelo padrão: `ollama pull phi3`.

### Instalação Manual (Linux / Windows CLI)

1. Certifique-se de ter o [Ollama](https://ollama.com/) instalado e execute:
```bash
ollama pull phi3

```


2. Mova o executável `vio` para o seu diretório de binários (ex: `/usr/local/bin` ou adicione ao `PATH`).
3. Crie o diretório de regras `config_rules/` no mesmo nível do binário ou em `/etc/vio/config_rules`.

---

## Passo a Passo de Uso

### Passo 1: Organizar a Estrutura do Projeto

O VIO usa o nome do diretório alvo como a `{SIGLA}` do projeto. Crie ou estruture seu projeto da seguinte forma:

```text
MEUPROJETO/
├── 1.Requisitos/
│   ├── MEUPROJETO - Visão.docx
│   └── Prototipo/
│       └── MEUPROJETO - Prototipo.docx
├── 2.Analise e Design/
│   └── MEUPROJETO - Modelos Analise e Design.uml
└── 6.Gerenciamento de Projeto/
    ├── MEUPROJETO - Planejamento e Controle do Projeto.xlsx
    └── Atas/
        └── MEUPROJETO - Ata de Reunião.docx

```

### Passo 2: Configurar as Regras

Coloque o arquivo de regras (ex: `regras_engenharia.json`) dentro da pasta `config_rules/` ao lado do executável ou na pasta de execução.

### Passo 3: Executar a Auditoria

Abra o terminal e aponte para a pasta do projeto:

```bash
vio ./MEUPROJETO

```

Para acompanhar em segundo plano com indicador de carregamento limpo:

```bash
vio --toggle-quiet
vio ./MEUPROJETO

```

### Passo 4: Gerar Dashboards e Exportações

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
* `arquivo_config`: Nome do arquivo JSON específico dentro de `config_rules/` (opcional; se omitido, executa todos os `.json` da pasta).

### Opções e Flags

| Flag | Descrição | Exemplo de Uso |
| --- | --- | --- |
| `--gerar-relatorio` | Gera relatório consolidado lendo os dados salvos no SQLite | `vio --gerar-relatorio` |
| `--html` | Exporta relatório em painel HTML com gráficos Chart.js | `vio --gerar-relatorio --html` |
| `--exportar-csv` | Exporta relatório em planilha CSV estruturada | `vio --gerar-relatorio --exportar-csv` |
| `--pasta-saida <DIR>` | Define pasta customizada de saída para CSV/HTML | `vio --gerar-relatorio --html --pasta-saida ./docs` |
| `--projeto <NOME>` | Filtra os registros do banco pelo nome da sigla | `vio --mostrar-banco --projeto MEUPROJ` |
| `--fase <JSON>` | Filtra os registros do banco pelo arquivo de regras | `vio --gerar-relatorio --fase sprint1.json` |
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
* **`pastas_ignoradas_extracao`** *(lista de strings, opcional)*: Pastas que a busca recursiva de "arquivos estranhos" deve pular. Ex: `[".git", "node_modules", "target"]`.
* **`regras_conformidade`** *(lista de objetos)*:
* **`id_regra`** *(string)*: Identificador legível ou pergunta da regra.
* **`tipo_arquivo`** *(string)*: Formato de extração. Aceita `"word"`, `"excel"`, `"astah"` ou `"uml"`.
* **`pasta_esperada`** *(string)*: Subdiretório onde o artefato deve estar localizado.
* **`nome_arquivo_base`** *(string)*: Nome do arquivo sem extensão. O curinga `{SIGLA}` é resolvido automaticamente pelo nome da pasta do projeto.
* **`obrigatorio`** *(boolean)*: Se `true` e o arquivo não for encontrado, marca como reprovado ("Não"). Se `false`, marca como "Não se aplica".
* **`apenas_existencia`** *(boolean, opcional)*: Se `true`, valida apenas a existência do arquivo, sem chamar o modelo de IA.
* **`aba_planilha`** *(string, opcional)*: Nome da aba da planilha caso o tipo seja `"excel"`.
* **`prompt_ia`** *(string)*: Prompt de auditoria enviado ao SLM.



---

## Atenção Crítica: O Loop Infinito da IA

O motor do VIO executa um laço `loop { ... }` que só é interrompido quando a resposta do modelo satisfaz **estritamente** a Expressão Regular compilada no código Rust:

```rust
(?is)(?:Análise|Analise|ANALISAÇÃO).*?:\s*(.*?)\s*Desvio.*?:\s*(.*?)\s*(?:Sugestão|Sugestao|SUGÉSTAO|SUGÉSTIMO).*?:\s*(.*?)\s*Status Qualitativo.*?:\s*[*_]*\s*(Sim|Não|Nao|Parcialmente)

```

> **Aviso Importante:** Se o campo `prompt_ia` for formulado de maneira livre ou não exigir a estrutura exata, o modelo retornará um formato diferente, o regex não capturará os grupos, e o VIO entrará em repetição contínua contra a API do Ollama.

### Template Obrigatório do `prompt_ia`

Todo prompt semântico deve terminar com as seguintes diretrizes de formatação:

```text
RESPONDA ESTRITAMENTE NESTE FORMATO:
Análise: [Breve contextualização de até 3 frases]
Desvio: [Problema identificado ou 'Nenhum']
Sugestão: [Ação corretiva ou 'Nenhuma']
Status Qualitativo: [Sim / Não / Parcialmente]

```

### Exemplo de Configuração JSON Válida

```json
{
  "endpoint_slm": "http://localhost:11434/api/generate",
  "modelo_slm": "phi3",
  "pastas_ignoradas_extracao": [
    ".git",
    "target",
    "3.Implementacao"
  ],
  "regras_conformidade": [
    {
      "id_regra": "O escopo e objetivos estão formalizados no documento de Visão?",
      "tipo_arquivo": "word",
      "pasta_esperada": "1.Requisitos",
      "nome_arquivo_base": "{SIGLA} - Visão",
      "obrigatorio": true,
      "prompt_ia": "Aja como auditor de software. Avalie o texto do documento Visão. Responda se os objetivos do produto estão bem delimitados. RESPONDA ESTRITAMENTE NESTE FORMATO:\nAnálise: [3 frases sobre a clareza do escopo]\nDesvio: [Descreva o desvio ou 'Nenhum']\nSugestão: [Sugestão ou 'Nenhuma']\nStatus Qualitativo: [Sim / Não / Parcialmente]"
    },
    {
      "id_regra": "A planilha de planejamento define a alocação de membros?",
      "tipo_arquivo": "excel",
      "pasta_esperada": "6.Gerenciamento de Projeto",
      "nome_arquivo_base": "{SIGLA} - Planejamento",
      "aba_planilha": "Equipe",
      "obrigatorio": true,
      "prompt_ia": "Aja como auditor. Analise os dados da aba de equipe e valide se os responsáveis foram atribuídos. RESPONDA ESTRITAMENTE NESTE FORMATO:\nAnálise: [3 frases avaliando a equipe]\nDesvio: [Descreva a inconformidade ou 'Nenhum']\nSugestão: [Recomendação ou 'Nenhuma']\nStatus Qualitativo: [Sim / Não / Parcialmente]"
    },
    {
      "id_regra": "O arquivo de protótipo de alta fidelidade está na pasta correta?",
      "tipo_arquivo": "word",
      "pasta_esperada": "1.Requisitos/Prototipos",
      "nome_arquivo_base": "{SIGLA} - Wireframes",
      "obrigatorio": false,
      "apenas_existencia": true,
      "prompt_ia": ""
    }
  ]
}

```

---

## Estrutura do Banco de Dados SQLite e Logs

O VIO armazena os dados no diretório de dados locais do sistema operacional (`%LOCALAPPDATA%\VIO\auditorias.db` no Windows ou no diretório do binário no Linux).

### Tabelas Internas

#### `identidade_projeto`

Armazena o perfil semântico sintetizado do sistema com base na regra de Visão:

* `projeto` (TEXT PRIMARY KEY): Sigla do projeto.
* `resumo_identidade` (TEXT NOT NULL): Resumo produzido pelo SLM para mitigar plágio/desvios em regras subsequentes.

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
<DIRETORIO_EXECUTAVEL>/logs/auditoria_erros.log

```
