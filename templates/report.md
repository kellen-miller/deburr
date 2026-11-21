{%- macro items(list, first) -%}
| # | Item | Why | Where |
|--:|---|---|---|
{% for item in list -%}
| {{ first + loop.index0 }} | **{{ self.kind_word(item.kind) }}** {{ item.title|md }}{% if item.category == Category::Test %} _(test)_{% endif %}{% if item.deferred %} _(deferred)_{% endif %} | {{ item.why|md }}{% if let Some(advice) = item.advice %}<br>{{ advice|md }}{% endif %} | {{ self.places(item.locations) }} |
{% endfor -%}
{%- endmacro -%}
## deburr · {{ summary.name|md }}

| | |
|---|---|
{% for (label, fact) in self.verdict() -%}
| **{{ label }}** | {{ fact.headline|md }}{% if !fact.detail.is_empty() %} — {{ fact.detail|md }}{% endif %} |
{% endfor %}
{% for hint in summary.hints -%}
> {{ hint|md }}

{% endfor -%}
{% if summary.worklist.is_empty() -%}
No open items.
{% else -%}
### Start here

{% call items(self.shown(), 1) %}{% endcall -%}
{% endif -%}
{% if !self.rest().is_empty() %}
<details><summary>{{ self.rest_heading() }}</summary>

{% call items(self.rest(), self.shown().len() + 1) %}{% endcall %}
</details>
{% endif -%}
{% if summary.settled > 0 %}
{{ summary.settled|count }} {% if summary.settled == 1 %}item{% else %}items{% endif %} settled in review.
{% endif -%}
{% if !summary.concentration.is_empty() %}
### Where it concentrates

| Directory | Complexity weight | Over limit | Copied lines | Code lines |
|---|--:|--:|--:|--:|
{% for load in self.directories() -%}
| {{ load.directory|md }} | {{ load.weight|count }} | {{ load.over_limit|count }} | {{ load.copied_lines|count }} | {{ load.code_lines|count }} |
{% endfor -%}
{% endif %}
---
deburr {{ report.analyzer.version|md }}{% if !summary.commit.is_empty() %} · {{ summary.commit|md }}{% endif %} <!-- deburr report schema {{ report.schema_version }} -->

