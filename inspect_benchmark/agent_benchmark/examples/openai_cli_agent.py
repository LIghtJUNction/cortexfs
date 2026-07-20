#!/usr/bin/env python3
"""
Example CLI agent for testing the bridge adapter.
It expects arguments:
  --prompt "..."

And uses OpenAI-compatible endpoint from OPENAI_BASE_URL.
The base URL is replaced by Inspect when running through the cli_bridge_agent.
"""

import argparse
import os
from openai import OpenAI


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--prompt", required=True)
    args = parser.parse_args()

    client = OpenAI(
        base_url=os.getenv("OPENAI_BASE_URL", "http://localhost:13131/v1"),
        api_key=os.getenv("OPENAI_API_KEY", "no-key"),
    )

    response = client.chat.completions.create(
        model="inspect",
        messages=[
            {
                "role": "user",
                "content": args.prompt,
            }
        ],
    )
    print(response.choices[0].message.content or "")


if __name__ == "__main__":
    main()
