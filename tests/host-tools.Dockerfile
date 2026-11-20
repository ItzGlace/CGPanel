FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates nginx bind9-utils mariadb-server postgresql python3 python3-pymysql python3-psycopg2 python3-sqlparse \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /project
