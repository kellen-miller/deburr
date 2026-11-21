{%- macro rows(marker, list) -%}
{% for entry in list -%}
| {{ marker }} | **{{ self.kind_word(entry.kind) }}** {{ entry.title|md }}{% if let Some(location) = entry.location %}<br>{{ self.place(location) }}{% endif %} | {{ entry.before|md }} | {{ entry.after|md }} |
{% endfor -%}
{%- endmacro -%}
### This pull request

**{{ summary.verdict|md }}**
{% if !summary.worse.is_empty() || !summary.better.is_empty() %}
| | Item | Before | After |
|---|---|---|---|
{% endif -%}
{% call rows("🔺", self.shown(summary.worse)) %}{% endcall -%}
{% call rows("✅", self.shown(summary.better)) %}{% endcall -%}
{% if self.hidden(summary.worse) > 0 %}
🔺 … {{ self.hidden(summary.worse) }} more
{% endif -%}
{% if self.hidden(summary.better) > 0 %}
✅ … {{ self.hidden(summary.better) }} more
{% endif -%}
{% if summary.unmatched > 0 %}
{{ summary.unmatched|count }} {% if summary.unmatched == 1 %}item{% else %}items{% endif %} could not be matched between the reports.
{% endif -%}
