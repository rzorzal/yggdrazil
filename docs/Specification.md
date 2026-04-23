Esta é a especificação técnica final e refinada para o **Yggdrazil (`ygg`)**, estruturada como um documento de entrada para ser consumido por um agente de codificação (como o Claude Code) para iniciar o desenvolvimento em Rust.

---

# Yggdrazil (ygg): The Agent Governance & Parallel World Engine

**Yggdrazil** é um orquestrador de governança local para agentes de IA (Claude Code, OpenAI Codex, Aider). Ele utiliza **Git Worktrees** para criar "Mundos" (Worlds) isolados a partir de um "Tronco" (Trunk/Main Repo), garantindo que múltiplos agentes trabalhem em paralelo sem colisões físicas e com consciência lógica compartilhada para otimização de tokens.

## 1. Filosofia de Governança
Diferente de orquestradores que "chamam" a IA, o Yggdrazil **governa o ambiente**. Ele configura as leis do repositório de forma que, quando qualquer agente for executado manualmente pelo usuário em um "Mundo", ele seja obrigado a ler as diretrizes e respeitar o estado dos outros agentes .

## 2. CLI API Specification (`ygg`) - Rust Core

O binário será desenvolvido em Rust para garantir performance na varredura de processos e segurança de memória .

```bash
# Instalação via GitHub (Script versionado)
curl -sSL https://raw.githubusercontent.com/<seu-user>/yggdrazil/main/scripts/install.sh | sh

# Inicialização (Simples)
# Detecta o repositório, converte para estrutura de worktrees se necessário e cria a pasta.ygg/
ygg init

# Inicialização (Segmentada)
# Cria um mundo específico com regras customizadas para aquela área do código
ygg init --world feature-auth --rules./docs/security_v2.md

# Dashboard de Monitoramento (TUI)
# Varre o sistema para encontrar processos de IA e mapeia em quais mundos eles estão
ygg monit

# Sincronização de Consciência
# Consolida os logs de todos os mundos no arquivo de memória compartilhada
ygg sync
```

## 3. Arquitetura Técnica

### A. The Roots (Sensor de Processos)
Utiliza a crate `sysinfo` para monitorar o sistema operacional de forma passiva .
- **Detecção:** Identifica binários como `claude-code`, `codex`, `aider`.
- **Mapeamento de Mundo:** Através do PID, o Ygg localiza o diretório de trabalho atual (`CWD`) do agente . Se o CWD estiver em `.ygg/worlds/`, o agente entra no dashboard de monitoramento.

### B. The Trunk (Gestão de Worktrees)
- O Yggdrazil automatiza o comando `git worktree add` para diretórios dentro de `.ygg/worlds/` .
- **Isolamento de Runtime:** Injeta dinamicamente variáveis de ambiente (como `PORT`) em cada mundo para evitar que dois agentes tentem subir o mesmo servidor na porta 3000 simultaneamente .

### C. The Laws (Injeção de Regras)
O `ygg init` cria links simbólicos ou arquivos de configuração obrigatórios em cada mundo (ex: `.cursorrules`, `CLAUDE.md`, `.aider.conf.yml`) .
- **Preâmbulo de Lei:** Todo arquivo de instrução começa com:
  > **YGGDRAZIL PROTOCOL ACTIVE**
  > 1. Você está no Mundo: `{{WORLD_ID}}`.
  > 2. O Agente `{{OTHER_AGENT}}` está trabalhando no arquivo `{{FILE}}`.
  > 3. Leia `.ygg/shared_memory.json` antes de agir para economizar tokens.

### D. Resonance Bus (Memória Compartilhada)
Para evitar o desperdício de tokens com redescobrimento, o Yggdrazil implementa um "Barramento de Ressonância" .
- **Conflict Warning:** Se o agente no Mundo 1 está editando um arquivo e o usuário abre um agente no Mundo 2 que tenta editar o mesmo arquivo, o `ygg monit` dispara um alerta ou injeta uma instrução de "Pausa" no arquivo de contexto do agente 2 .

## 4. Estratégia de Instalação e Release (GitHub)

Para evitar a necessidade de um domínio `.sh` próprio inicialmente, utilizaremos a infraestrutura do GitHub [1, 2]:

1.  **Releases:** O binário Rust será compilado via GitHub Actions para Linux, macOS e Windows.[1, 3]
2.  **install.sh:** Um script hospedado no repositório que:
    - Identifica a arquitetura (ex: `x86_64-apple-darwin`).
    - Consulta a API do GitHub para pegar a URL do *asset* da última versão estável.[2]
    - Baixa, extrai e move o binário para `/usr/local/bin/ygg`.

**URL de instalação sugerida:**
`https://raw.githubusercontent.com/<seu-user>/yggdrazil/main/scripts/install.sh`

## 5. Implementação: Roadmap para o Agente Rust

Para iniciar o desenvolvimento, você pode passar este prompt ao seu agente:

> *"Atue como um Engenheiro de Sistemas Sênior em Rust. Vamos construir o Yggdrazil conforme a especificação. Siga estes passos iniciais:*
> 1. *Crie o projeto com `clap` para comandos CLI e `sysinfo` para o monitoramento.*
> 2. *Implemente o `ygg init` que cria a pasta `.ygg/worlds/` e adiciona um Git Worktree básico.*
> 3. *Crie um módulo de 'Sensor' que liste processos com nome 'claude' ou 'aider' e imprima o diretório onde estão rodando.*
> 4. *Configure o GitHub Actions para compilar o projeto em cada commit de tag."*

Esta estrutura garante que o Yggdrazil seja um "Professor" silencioso, mas onipresente, que organiza o caos do desenvolvimento paralelo por IAs .
