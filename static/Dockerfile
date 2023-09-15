FROM scratch
# copy server binary from build stage
#COPY --from=builder /code/target/release/RatioUp /app/RatioUp
ADD static /app/static
COPY RatioUp /app/
COPY Docker.env /app/.env

WORKDIR /app
VOLUME /torrents

LABEL author="Slundi"
LABEL url="https://github.com/slundi/RatioUp"
LABEL vcs-url="https://github.com/slundi/RatioUp"
# set user to non-root unless root is required for your app
# USER 1001
ENTRYPOINT [ "/app/RatioUp" ]

EXPOSE 8070/tcp
