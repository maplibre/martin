FROM node:alpine@sha256:cf38e1f3c28ac9d81cdc0c51d8220320b3b618780e44ef96a39f76f7dbfef023 AS builder

WORKDIR /usr/src/app

COPY package.json .
COPY yarn.lock .
RUN yarn install

COPY . .
RUN yarn run build

FROM nginx:alpine@sha256:e7257f1ef28ba17cf7c248cb8ccf6f0c6e0228ab9c315c152f9c203cd34cf6d1
COPY nginx.conf /etc/nginx/nginx.conf
COPY --from=builder /usr/src/app/dist /usr/share/nginx/html
