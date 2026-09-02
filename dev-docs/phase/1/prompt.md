I want a simple project named omoffice. It will for now consist of a sample API.

Stack: Terraform manages AWS resources. Rust backend, deployed as lambda and api gateway

Usage:

```
# export env vars (including AWS_PROFILE)

export $(grep -v '^#' .env | xargs -d '\n')

cd backend

make init # installs requirements

make run dev # outputs localhost:8000

curl localhost:8000 # {message: "hello world!"}

cd ../infra
make deploy-dev # error: unitialized
make init # installs terraform, starts project
make deploy dev # outputs all resources

echo "copy values to <some backend file>"

cd ../backend

make deploy dev

curl <api-url> # {message: "hello worvld!"}

```