CONFIG_DIR="/usr/local/etc/martin.d"



if [ -d "${CONFIG_DIR}" ]
then
  echo "==> config directory '${CONFIG_DIR}' should be manually removed."
  echo "  rm -rf ${CONFIG_DIR}"
fi

if [ -d "/var/run/martin" ]
then
  echo "==> run directory '/var/run/martin' should be manually removed."
  echo "  rm -rf /var/run/martin"
fi
