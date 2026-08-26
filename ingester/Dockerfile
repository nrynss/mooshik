# Mooshik bootstrap ingester (M8). Deploy step pending IAM setup — see README.
FROM python:3.13-slim

WORKDIR /app

COPY pyproject.toml README.md ./
COPY ingester ./ingester

RUN pip install --no-cache-dir .

# Corpus root is mounted or baked at deploy time; state goes to a volume.
CMD ["python3", "-m", "ingester", "--root", "/corpus"]
