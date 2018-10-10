FROM node:alpine
LABEL maintainer="Andrey Bakhvalov<bakhvalov.andrey@gmail.com>"

WORKDIR /usr/src/app
COPY package.json /usr/src/app/package.json
COPY yarn.lock /usr/src/app/yarn.lock

RUN yarn

CMD [ "yarn", "start" ]
