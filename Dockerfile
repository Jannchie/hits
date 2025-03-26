# Use the latest stable Rust image as the base image for building the application
FROM rust:1.85 as builder

# Set the working directory inside the container
WORKDIR /usr/src/hits

# Install sqlx-cli with minimal features
RUN cargo install sqlx-cli --no-default-features --features postgres

# Copy the Cargo.toml and Cargo.lock files to leverage Docker's caching mechanism
COPY Cargo.toml Cargo.lock ./

# Build the dependencies
RUN cargo build --release && rm -f target/release/deps/hits*

# Copy the source code
COPY . .

# Build the application
RUN cargo build --release

# Use a smaller base image for the final stage
FROM debian:buster-slim


# Set the working directory inside the container
WORKDIR /usr/src/hits

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/hits/target/release/hits .

# Copy sqlx-cli from the builder stage
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx

# Expose the port the application runs on
EXPOSE 3030

# Run the application
CMD ["./hits"]