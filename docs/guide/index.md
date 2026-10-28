# SpaceStorage guide

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/logo-on-dark.svg">
  <img alt="SpaceStorage" src="images/logo.svg" width="72" height="72">
</picture>

Operator documentation for the server in this repository. The product tip is [`docs/milestones/009-full-product`](../milestones/009-full-product.md).

| Page | What it covers |
|------|----------------|
| [Product](product.md) | What the server is, which protocols it speaks, and the three build profiles |
| [Architecture](architecture.md) | Process, datatype ladder, placement, and durability |
| [Using the server](using.md) | Build, start, validate, clients, and the admin CLI |
| [Configuration](configuration.md) | Grammar and every block the config parser accepts |

Diagrams on the architecture page are the pictures for this guide. Mermaid figures are the precise ones; the images are the same layout with short labels.

The older note [`docs/configuration.md`](../configuration.md) is the original runtime grammar. Blocks added after that note (`cluster`, `limits`, `storage`, and the rest) are in [Configuration](configuration.md) here.
