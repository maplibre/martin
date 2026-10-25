# The OGC API - Maps conformance suite, built from source as it has no published image yet.
FROM maven:3.9-eclipse-temurin-17@sha256:1a352420f7aba21f5ad08df31bab55f74c013fb491f1ae8ab1dd7ff9ed698584 AS build
ARG ETS_COMMIT=83d256077c8dc620c0945d84f44306cd6b7783a2
WORKDIR /ets
RUN git init -q \
 && git fetch -q --depth 1 https://github.com/opengeospatial/ets-ogcapi-maps10.git "$ETS_COMMIT" \
 && git checkout -q FETCH_HEAD \
 && mvn -B -q -DskipTests -Pdocker -Ddocker.skip=true package
RUN mkdir -p /out/teamengine /out/lib /out/te_base/scripts \
 && cd /out/teamengine && jar xf /ets/target/dependency/teamengine-web-*[0-9].war \
 && cd /out/lib && jar xf /ets/target/dependency/teamengine-web-*-common-libs.zip \
 && cd /out/te_base && jar xf /ets/target/dependency/teamengine-console-*-base.zip \
 && cd /out/te_base/scripts && jar xf /ets/target/ets-ogcapi-maps10-*-ctl.zip \
 && cd /out/teamengine/WEB-INF/lib && jar xf /ets/target/ets-ogcapi-maps10-*-deps.zip \
 && rm -r /out/te_base/scripts/note

FROM tomcat:10.1-jre17@sha256:f09917708e4be97d2ab9fa837182785b4dc0e704f9fab47885bea6fdd008a56e
COPY --from=build /out/teamengine /usr/local/tomcat/webapps/teamengine
COPY --from=build /out/lib /usr/local/tomcat/lib
COPY --from=build /out/te_base /root/te_base
RUN sed -i -e 's/port="8080"/port="8081"/' -e 's/port="8005"/port="-1"/' /usr/local/tomcat/conf/server.xml
ENV JAVA_OPTS="-Xms1024m -Xmx2048m -DTE_BASE=/root/te_base"
CMD ["catalina.sh", "run"]
