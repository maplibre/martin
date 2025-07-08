FROM node:alpine AS builder

WORKDIR /usr/src/app

COPY package.json .
COPY yarn.lock .
RUN yarn install

COPY . .
RUN yarn run build

CMD ["yarn", "run", "preview"]
